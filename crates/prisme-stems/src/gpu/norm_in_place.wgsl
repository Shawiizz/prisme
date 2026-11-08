// In-place RMS normalisation of rows, times a gain (a transformer's output norm).

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

@group(0) @binding(0) var<storage, read_write> x: array<f32>;
@group(0) @binding(1) var<storage, read> gain: array<f32>;
@group(0) @binding(2) var<uniform> p: Params;

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
fn normalize_in_place(@builtin(workgroup_id) wg: vec3<u32>, @builtin(local_invocation_index) lid: u32) {
    let row = wg.x + wg.y * p.groups_x;
    if (row >= p.rows) {
        return;
    }
    let start = p.src_off + row * p.src_ld;
    var sum = 0.0;
    for (var i = lid; i < p.width; i += THREADS) {
        let v = x[start + i];
        sum += v * v;
    }
    let norm = reduce(lid, sum);
    for (var i = lid; i < p.width; i += THREADS) {
        x[start + i] = x[start + i] / norm * gain[i];
    }
}
