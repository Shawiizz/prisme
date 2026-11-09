// dst = src, or dst += src: the sums of the skip connections between transformer pairs.

struct Params {
    len: u32,
    accumulate: u32,
    groups_x: u32,
}

@group(0) @binding(0) var<storage, read> src: array<f32>;
@group(0) @binding(1) var<storage, read_write> dst: array<f32>;
@group(0) @binding(2) var<uniform> p: Params;

@compute @workgroup_size(256)
fn main(@builtin(workgroup_id) wg: vec3<u32>, @builtin(local_invocation_index) lid: u32) {
    let i = (wg.x + wg.y * p.groups_x) * 256u + lid;
    if (i >= p.len) {
        return;
    }
    if (p.accumulate == 1u) {
        dst[i] += src[i];
    } else {
        dst[i] = src[i];
    }
}
