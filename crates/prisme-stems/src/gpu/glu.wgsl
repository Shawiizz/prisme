// The masks closing a band's estimator: out = a * sigmoid(gate), the estimator's output being
// per row `width` values of `a` then a gate for each (a GLU), or a gate for each pair of real
// and imaginary parts, or no gates (`a` alone). Its rows come by tokens of `group` frames,
// the first frame of every token first (`tokens` of them), then the second's...; the masks'
// frame after frame.

struct Params {
    rows: u32,
    width: u32,
    dst_ld: u32,
    dst_off: u32,
    groups_x: u32,
    // 0: none, 1: one per value, 2: one per pair
    gates: u32,
    group: u32,
    tokens: u32,
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
    var gates = 0u;
    var gate = 0u;
    if (p.gates == 1u) {
        gates = p.width;
        gate = j;
    } else if (p.gates == 2u) {
        gates = p.width / 2u;
        gate = j / 2u;
    }
    let row = ((r % p.group) * p.tokens + r / p.group) * (p.width + gates);
    let a = y[row + j];
    var m = a;
    if (gates != 0u) {
        let g = y[row + p.width + gate];
        m = a / (1.0 + exp(-g));
    }
    mask[p.dst_off + r * p.dst_ld + j] = m;
}
