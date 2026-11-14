// The gated feed-forward's hidden features, in place of its gates: each row holds `width`
// gates then `width` values, and its gates become gelu(gate) * value.

struct Params {
    rows: u32,
    width: u32,
    groups_x: u32,
}

@group(0) @binding(0) var<storage, read_write> hidden: array<Act>;
@group(0) @binding(1) var<uniform> p: Params;

fn erf(x: f32) -> f32 {
    // Abramowitz & Stegun 7.1.26, |error| < 1.5e-7
    let t = 1.0 / (1.0 + 0.3275911 * abs(x));
    let y = 1.0 - ((((1.061405429 * t - 1.453152027) * t + 1.421413741) * t - 0.284496736) * t + 0.254829592) * t * exp(-x * x);
    return select(-y, y, x >= 0.0);
}

@compute @workgroup_size(256)
fn main(@builtin(workgroup_id) wg: vec3<u32>, @builtin(local_invocation_index) lid: u32) {
    let id = (wg.x + wg.y * p.groups_x) * 256u + lid;
    if (id >= p.rows * p.width) {
        return;
    }
    let at = id / p.width * 2u * p.width + id % p.width;
    let gate = f32(hidden[at]);
    hidden[at] = Act(0.5 * gate * (1.0 + erf(gate * 0.70710678118654752)) * f32(hidden[at + p.width]));
}
