// The gated linear unit closing a band's mask estimator: out = a * sigmoid(gate), the
// estimator's output being `width` values of `a` then `width` gates per row.

struct Params {
    rows: u32,
    width: u32,
    dst_ld: u32,
    dst_off: u32,
    groups_x: u32,
}

@group(0) @binding(0) var<storage, read> y: array<f32>;
@group(0) @binding(1) var<storage, read_write> mask: array<f32>;
@group(0) @binding(2) var<uniform> p: Params;

@compute @workgroup_size(256)
fn main(@builtin(workgroup_id) wg: vec3<u32>, @builtin(local_invocation_index) lid: u32) {
    let id = (wg.x + wg.y * p.groups_x) * 256u + lid;
    if (id >= p.rows * p.width) {
        return;
    }
    let r = id / p.width;
    let j = id % p.width;
    let a = y[r * 2u * p.width + j];
    let g = y[r * 2u * p.width + p.width + j];
    mask[p.dst_off + r * p.dst_ld + j] = a / (1.0 + exp(-g));
}
