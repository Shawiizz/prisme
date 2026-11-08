// RMS normalisation of rows, one workgroup per row: x / max(|x|, 1e-12), times a gain.

struct Params {
    rows: u32,
    width: u32,
    src_ld: u32,
    src_off: u32,
    dst_ld: u32,
    dst_off: u32,
    has_gain: u32,
    groups_x: u32,
}

@group(0) @binding(0) var<storage, read> src: array<f32>;
@group(0) @binding(1) var<storage, read_write> dst: array<f32>;
@group(0) @binding(2) var<storage, read> gain: array<f32>;
@group(0) @binding(3) var<uniform> p: Params;

const THREADS: u32 = 128u;
var<workgroup> partial: array<f32, THREADS>;

fn reduce(lid: u32, value: f32) -> f32 {
    partial[lid] = value;
    workgroupBarrier();
    for (var s = THREADS / 2u; s > 0u; s >>= 1u) {
        if (lid < s) {
            partial[lid] += partial[lid + s];
        }
        workgroupBarrier();
    }
    return max(sqrt(partial[0]), 1e-12);
}

@compute @workgroup_size(128)
fn normalize(@builtin(workgroup_id) wg: vec3<u32>, @builtin(local_invocation_index) lid: u32) {
    let row = wg.x + wg.y * p.groups_x;
    if (row >= p.rows) {
        return;
    }
    let start = p.src_off + row * p.src_ld;
    var sum = 0.0;
    for (var i = lid; i < p.width; i += THREADS) {
        let v = src[start + i];
        sum += v * v;
    }
    let norm = reduce(lid, sum);
    let to = p.dst_off + row * p.dst_ld;
    for (var i = lid; i < p.width; i += THREADS) {
        var v = src[start + i] / norm;
        if (p.has_gain != 0u) {
            v *= gain[i];
        }
        dst[to + i] = v;
    }
}
