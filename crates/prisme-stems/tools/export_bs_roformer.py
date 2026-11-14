"""Convert a BS-RoFormer of HiDolen's Mini-BS-RoFormer family into the safetensors file Prisme reads.

    python export_bs_roformer.py [--half] [--stems a,b] [--overlap N] MODEL_DIR out.safetensors
    python export_bs_roformer.py --fixture ../tests/data/tiny_bs_roformer.safetensors MODEL_DIR

MODEL_DIR is the model's Hugging Face directory (config.json, model.safetensors and its
modeling code, the reference). Needs torch, safetensors, einops and transformers.

The file holds the network between the STFT and the iSTFT, laid out as `export_roformer.py`
lays out the Mel-Band RoFormers, with what sets this family apart in the metadata:

* the keys and values have fewer heads than the queries (`kv_heads`), the heads no gates
  (`gates=0`);
* the RMS norms add an epsilon (`norm_eps`), and their gains, times the square root of their
  width, are folded into the linear layers they feed;
* the feed-forwards are gated (`ff_glu`): their first product gives the gates, then the
  values;
* the transformers have no output norms; one norm follows the last (`final_norm`);
* a square of learned tokens (`registers`) follows the last frame and band;
* the mask estimators are one linear layer per band, without a GLU (`mask_glu=0`).

The rotary embedding rotates the halves of each head, where Prisme rotates its pairs: the
query and key rows of each head are interleaved so that its pairs are the halves' elements.
The attention scale is folded into the query weights.

`--half`, `--stems` and `--overlap` are as in `export_roformer.py`. `--fixture` writes a tiny
model with random weights, a random input and the reference model's output (`test.*`).
"""

import argparse
import json
import math
import sys
from pathlib import Path

import torch
from safetensors.torch import save_file


def reference(model_dir, config=None):
    """HiDolen's model from its directory: its weights, or random ones for `config` (a dict
    overriding config.json)."""
    from transformers import AutoConfig, AutoModel

    if config is None:
        return AutoModel.from_pretrained(model_dir, trust_remote_code=True).eval()
    cfg = AutoConfig.from_pretrained(model_dir, trust_remote_code=True)
    for k, v in config.items():
        setattr(cfg, k, v)
    return AutoModel.from_config(cfg, trust_remote_code=True).eval()


def gain(norm):
    """What an nn.RMSNorm multiplies the unit vector by: its weight, times sqrt(width)."""
    return norm.weight.detach() * math.sqrt(norm.weight.shape[0])


def halves_to_pairs(rows, heads, dim_head):
    """Rows of `heads` heads, each head's halves interleaved: element i, then i + dim_head / 2."""
    half = dim_head // 2
    order = torch.stack([torch.arange(half), torch.arange(half) + half], dim=1).flatten()
    return rows.view(heads, dim_head, -1)[:, order].reshape(heads * dim_head, -1)


def tensors_of(model, stems):
    """`stems`: (name, index) of the mask estimators to keep."""
    cfg = model.config
    body = model.model
    dim = cfg.hidden_size
    heads, kv_heads = cfg.num_attention_heads, cfg.num_key_value_heads
    dim_head = dim // heads
    freqs = cfg.stft_n_fft // 2 + 1
    assert cfg.num_input_channel == 2, "Prisme separates stereo"
    assert cfg.stft_win_length == cfg.stft_n_fft, "Prisme's window spans the whole FFT"
    assert cfg.mask_estimator_depth == 1 and all(
        isinstance(m, torch.nn.Linear) for e in model.mask_estimators for m in e.to_freq_mlps
    ), "the mask estimators are one linear layer per band"

    t = {}
    # the features of a frame are the bins' (re, im), channel after channel, bin after bin
    t["freq_indices"] = torch.arange(2 * freqs, dtype=torch.int32)
    t["bands_per_freq"] = torch.ones(freqs, dtype=torch.int32)
    inv_freq = body.rotary_emb.inv_freq.detach().float()
    t["rotary.time"] = inv_freq
    t["rotary.freq"] = inv_freq.clone()
    for b, (norm, linear) in enumerate(body.band_split.to_features):
        t[f"band.{b}.weight"] = linear.weight.detach() * gain(norm)[None, :]
        t[f"band.{b}.bias"] = linear.bias.detach()
    transformers = [tr for pair in body.layers for tr in pair]
    for i, tr in enumerate(transformers):
        for j, layer in enumerate(tr.layers):
            p = f"transformer.{i}.layer.{j}"
            attn, mlp = layer.self_attn, layer.mlp
            assert attn.q_proj.bias is None and mlp.gate_proj.bias is None, "no biases in the transformers"
            g = gain(layer.input_layernorm)[None, :]
            q = halves_to_pairs(attn.q_proj.weight.detach(), heads, dim_head) * attn.scaling
            k = halves_to_pairs(attn.k_proj.weight.detach(), kv_heads, dim_head)
            v = attn.v_proj.weight.detach()
            t[f"{p}.qkvg.weight"] = torch.cat([q, k, v]) * g
            t[f"{p}.out.weight"] = attn.o_proj.weight.detach()
            g = gain(layer.post_attention_layernorm)[None, :]
            t[f"{p}.ff1.weight"] = torch.cat([mlp.gate_proj.weight, mlp.up_proj.weight]).detach() * g
            t[f"{p}.ff2.weight"] = mlp.down_proj.weight.detach()
    t["final_norm"] = gain(body.final_norm)
    t["registers"] = body.register_tokens.detach().flatten()
    for name, index in stems:
        for b, linear in enumerate(model.mask_estimators[index].to_freq_mlps):
            t[f"mask.{name}.{b}.0.weight"] = linear.weight.detach()
            t[f"mask.{name}.{b}.0.bias"] = linear.bias.detach()
    meta = dict(
        dim=dim,
        heads=heads,
        kv_heads=kv_heads,
        dim_head=dim_head,
        gates=0,
        time_depth=cfg.time_transformer_depth,
        freq_depth=cfg.freq_transformer_depth,
        depth=cfg.depth,
        mask_layers=1,
        mask_glu=0,
        ff_glu=1,
        norm_eps=cfg.rms_norm_eps,
        registers=cfg.register_token_num,
        n_fft=cfg.stft_n_fft,
        hop=cfg.stft_hop_length,
        stems=",".join(name for name, _ in stems),
    )
    return {k: v.contiguous().clone() for k, v in t.items()}, meta


def save(model, out, meta_extra, stems, extra_tensors=None, half=False):
    tensors, meta = tensors_of(model, stems)
    if half:
        tensors = {k: v.half() if k.endswith((".weight", ".bias")) else v for k, v in tensors.items()}
    meta.update(meta_extra)
    tensors.update(extra_tensors or {})
    save_file(tensors, out, metadata={k: str(v) for k, v in meta.items()})
    print(f"wrote {out}: {len(tensors)} tensors, {json.dumps(meta)}")


# the stems of the Mini-BS-RoFormers, in the order of their mask estimators (MUSDB18's)
STEMS = ["bass", "drums", "other", "vocals"]


def export(model_dir, out, half, stems, overlap, chunk_seconds):
    model = reference(model_dir)
    names = STEMS if model.config.num_stems == len(STEMS) else [f"stem{i}" for i in range(model.config.num_stems)]
    chosen = [(name, names.index(name)) for name in stems.split(",")] if stems else list(zip(names, range(len(names))))
    hop = model.config.stft_hop_length
    chunk = round(chunk_seconds * 44100)
    meta = dict(sample_rate=44100, chunk_frames=chunk // hop + 1, overlap=overlap)
    save(model, out, meta, chosen, half=half)


def fixture(model_dir, out):
    """Grouped heads, registers and two stems on a tiny model."""
    torch.manual_seed(11)
    model = reference(model_dir, dict(
        hidden_size=32, depth=2, num_input_channel=2, num_stems=2, time_transformer_depth=1,
        freq_transformer_depth=1, freqs_per_bands=[2] * 8 + [4] * 4 + [8] * 2 + [17],
        num_attention_heads=4, num_key_value_heads=2, intermediate_size=48, stft_n_fft=128,
        stft_hop_length=32, stft_win_length=128, mask_estimator_depth=1, register_token_num=2,
    ))
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
    ap.add_argument("model_dir")
    ap.add_argument("out", nargs="?")
    ap.add_argument("--fixture")
    ap.add_argument("--half", action="store_true")
    ap.add_argument("--stems")
    ap.add_argument("--overlap", type=int, default=1)
    ap.add_argument("--chunk-seconds", type=float, default=8.0, help="as the model was trained")
    a = ap.parse_args()
    if a.fixture:
        fixture(a.model_dir, a.fixture)
    elif a.out:
        export(a.model_dir, a.out, a.half, a.stems, a.overlap, a.chunk_seconds)
    else:
        sys.exit("an output file, or --fixture")
