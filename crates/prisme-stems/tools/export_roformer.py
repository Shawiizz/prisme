"""Convert a Mel-Band RoFormer checkpoint into the safetensors file Prisme reads.

    python export_roformer.py --msst DIR [--half] [--stems a,b] [--overlap N] model.ckpt model.yaml out.safetensors
    python export_roformer.py --msst DIR --fixture ../tests/data/tiny_roformer_stems.safetensors

Needs torch, safetensors, pyyaml, and a checkout of ZFTurbo's Music-Source-Separation-Training
(`--msst`), whose model class is the reference.

Prisme runs the network between the STFT and the iSTFT itself, so the file holds that part
only, laid out for it:

* every RMSNorm that feeds linear layers has its gain folded into their weights;
* the attention scale is folded into the query weights;
* the query/key/value projection and the attention gates form one matrix (`qkvg`).

`--half` stores the weights and biases in float16, halving the download: the output moves by
about 5e-4 of its level (-66 dB). Rotary frequencies, norm gains and indices stay exact.

`--stems` keeps the mask estimators of these stems only (all of them by default): a file
without the stems a caller derives from the others is smaller and faster. A model of one stem
written without `--stems` keeps the unnamed layout of the first vocals model.

`--overlap` is how many chunks cover each sample when Prisme separates a track (1 by default:
back to back).

`--fixture` writes a tiny model of two stems with random weights, plus a random input and the
reference model's output (`test.*` tensors): the Rust tests check their implementations
against it.
"""

import argparse
import json
import sys
from pathlib import Path

import torch
import yaml
from safetensors.torch import save_file


def fold(norm, weight):
    return (weight * (norm.scale * norm.gamma)[None, :]).detach()


def tensors_of(model, stems):
    """`stems`: (name, index) of the mask estimators to keep, or None for the unnamed layout."""
    t = {}
    heads = model.layers[0][0].layers[0][0].heads
    dim_head = model.layers[0][0].layers[0][0].to_qkv.weight.shape[0] // 3 // heads
    t["freq_indices"] = model.freq_indices.to(torch.int32)
    t["bands_per_freq"] = model.num_bands_per_freq.to(torch.int32)
    t["rotary.time"] = model.layers[0][-2].layers[0][0].rotary_embed.freqs.detach().float()
    t["rotary.freq"] = model.layers[0][-1].layers[0][0].rotary_embed.freqs.detach().float()
    for b, net in enumerate(model.band_split.to_features):
        norm, linear = net
        t[f"band.{b}.weight"] = fold(norm, linear.weight)
        t[f"band.{b}.bias"] = linear.bias.detach()
    assert all(len(block) == 2 for block in model.layers), "linear transformers are not supported"
    transformers = [tr for pair in model.layers for tr in pair]
    for i, tr in enumerate(transformers):
        for j, (attn, ff) in enumerate(tr.layers):
            p = f"transformer.{i}.layer.{j}"
            qkv = fold(attn.norm, attn.to_qkv.weight)
            qkv[: heads * dim_head] *= dim_head**-0.5
            gates = fold(attn.norm, attn.to_gates.weight)
            t[f"{p}.qkvg.weight"] = torch.cat([qkv, gates])
            t[f"{p}.qkvg.bias"] = torch.cat([torch.zeros(qkv.shape[0]), attn.to_gates.bias.detach()])
            t[f"{p}.out.weight"] = attn.to_out[0].weight.detach()
            norm, lin1, _, _, lin2, _ = ff.net
            t[f"{p}.ff1.weight"] = fold(norm, lin1.weight)
            t[f"{p}.ff1.bias"] = lin1.bias.detach()
            t[f"{p}.ff2.weight"] = lin2.weight.detach()
            t[f"{p}.ff2.bias"] = lin2.bias.detach()
        t[f"transformer.{i}.norm"] = (tr.norm.scale * tr.norm.gamma).detach()
    for name, index in stems or [(None, 0)]:
        prefix = "mask." if name is None else f"mask.{name}."
        for b, mlp in enumerate(model.mask_estimators[index].to_freqs):
            linears = [m for m in mlp[0] if isinstance(m, torch.nn.Linear)]
            for k, linear in enumerate(linears):
                t[f"{prefix}{b}.{k}.weight"] = linear.weight.detach()
                t[f"{prefix}{b}.{k}.bias"] = linear.bias.detach()
    meta = dict(
        dim=model.band_split.to_features[0][1].out_features,
        heads=heads,
        dim_head=dim_head,
        time_depth=len(model.layers[0][0].layers),
        freq_depth=len(model.layers[0][1].layers),
        depth=len(model.layers),
        mask_layers=len(linears),
        n_fft=model.stft_kwargs["n_fft"],
        hop=model.stft_kwargs["hop_length"],
        skip_connection=int(model.skip_connection),
    )
    if stems:
        meta["stems"] = ",".join(name for name, _ in stems)
    return {k: v.contiguous().clone() for k, v in t.items()}, meta


def save(model, out, meta_extra, stems=None, extra_tensors=None, half=False):
    tensors, meta = tensors_of(model, stems)
    if half:
        tensors = {k: v.half() if k.endswith((".weight", ".bias")) else v for k, v in tensors.items()}
    meta.update(meta_extra)
    tensors.update(extra_tensors or {})
    save_file(tensors, out, metadata={k: str(v) for k, v in meta.items()})
    print(f"wrote {out}: {len(tensors)} tensors, {json.dumps(meta)}")


def export(ckpt, config, out, half, stems, overlap):
    from models.bs_roformer.mel_band_roformer import MelBandRoformer

    cfg = yaml.load(Path(config).read_text(), Loader=yaml.FullLoader)
    model = MelBandRoformer(**cfg["model"])
    state = torch.load(ckpt, map_location="cpu", weights_only=False)
    model.load_state_dict(state.get("state_dict", state))
    hop = cfg["model"]["stft_hop_length"]
    chunk = cfg["audio"]["chunk_size"] if "audio" in cfg else hop * (cfg["inference"]["dim_t"] - 1)
    meta = dict(sample_rate=cfg["model"]["sample_rate"], chunk_frames=chunk // hop + 1, overlap=overlap)
    instruments = cfg["training"]["instruments"]
    chosen = [(name, instruments.index(name)) for name in stems.split(",")] if stems else None
    if chosen is None and len(instruments) > 1:
        chosen = [(name, i) for i, name in enumerate(instruments)]
    save(model.eval(), out, meta, stems=chosen, half=half)


def fixture(out):
    """Two stems and skip connections: what the multi-stem models use."""
    from models.bs_roformer.mel_band_roformer import MelBandRoformer

    torch.manual_seed(7)
    model = MelBandRoformer(
        dim=32, depth=3, stereo=True, num_stems=2, time_transformer_depth=1, freq_transformer_depth=1,
        num_bands=8, dim_head=16, heads=2, attn_dropout=0.0, ff_dropout=0.0, flash_attn=False,
        dim_freqs_in=65, sample_rate=44100, stft_n_fft=128, stft_hop_length=32, stft_win_length=128,
        mask_estimator_depth=2, skip_connection=True, zero_dc=False,
    ).eval()
    with torch.no_grad():
        for p in model.parameters():
            p.add_(torch.randn_like(p) * 0.2)
        audio = torch.randn(1, 2, 32 * 20) * 0.3
        expected = model(audio)[0]
    save(model, out, dict(sample_rate=44100, chunk_frames=21, overlap=1),
         stems=[("drums", 0), ("bass", 1)],
         extra_tensors={"test.audio": audio[0].contiguous(), "test.stems": expected.contiguous()})


if __name__ == "__main__":
    ap = argparse.ArgumentParser()
    ap.add_argument("ckpt", nargs="?")
    ap.add_argument("config", nargs="?")
    ap.add_argument("out", nargs="?")
    ap.add_argument("--msst", required=True, help="checkout of Music-Source-Separation-Training")
    ap.add_argument("--fixture")
    ap.add_argument("--half", action="store_true")
    ap.add_argument("--stems")
    ap.add_argument("--overlap", type=int, default=1)
    a = ap.parse_args()
    sys.path.insert(0, a.msst)
    if a.fixture:
        fixture(a.fixture)
    else:
        export(a.ckpt, a.config, a.out, a.half, a.stems, a.overlap)
