// Gated multi-head attention along one axis, in one pass over the keys (online softmax):
// a workgroup takes 64 queries of one sequence and one head, each thread one query, and
// walks the keys and values 32 at a time through shared memory. Each head's output is
// scaled by the sigmoid of its gate. The queries come pre-scaled and rotated.

struct Params {
    len: u32,
    bands: u32,
    // 0: sequences along the frames (one per band), 1: along the bands (one per frame)
    axis: u32,
    width: u32,
    inner: u32,
    heads: u32,
    query_blocks: u32,
    // sequences * heads * query_blocks
    blocks: u32,
    groups_x: u32,
}

@group(0) @binding(0) var<storage, read> qkvg: array<f32>;
@group(0) @binding(1) var<storage, read_write> out: array<f32>;
@group(0) @binding(2) var<uniform> p: Params;

override DH: u32 = 64u;
const QUERIES: u32 = 64u;
const KEYS: u32 = 32u;
const MAX_DH: u32 = 64u;

var<workgroup> k_tile: array<f32, 2048>;
var<workgroup> v_tile: array<f32, 2048>;

fn token(group: u32, i: u32) -> u32 {
    return select(i * p.bands + group, group * p.bands + i, p.axis == 1u);
}

@compute @workgroup_size(64)
fn main(@builtin(workgroup_id) wg: vec3<u32>, @builtin(local_invocation_index) lid: u32) {
    let block = wg.x + wg.y * p.groups_x;
    if (block >= p.blocks) {
        return;
    }
    let query_block = block % p.query_blocks;
    let group_head = block / p.query_blocks;
    let head = group_head % p.heads;
    let group = group_head / p.heads;
    let i = query_block * QUERIES + lid;
    let valid = i < p.len;
    let me = token(group, min(i, p.len - 1u));

    var q: array<f32, MAX_DH>;
    var o: array<f32, MAX_DH>;
    for (var d = 0u; d < DH; d++) {
        q[d] = qkvg[me * p.width + head * DH + d];
        o[d] = 0.0;
    }
    var m = -3.0e38;
    var l = 0.0;
    var scores: array<f32, KEYS>;

    for (var k0 = 0u; k0 < p.len; k0 += KEYS) {
        for (var e = lid; e < KEYS * DH; e += QUERIES) {
            let key = k0 + e / DH;
            var kv = 0.0;
            var vv = 0.0;
            if (key < p.len) {
                let at = token(group, key) * p.width + head * DH + e % DH;
                kv = qkvg[at + p.inner];
                vv = qkvg[at + 2u * p.inner];
            }
            k_tile[e] = kv;
            v_tile[e] = vv;
        }
        workgroupBarrier();
        let n = min(KEYS, p.len - k0);
        var block_max = -3.0e38;
        for (var j = 0u; j < n; j++) {
            var s = 0.0;
            for (var d = 0u; d < DH; d++) {
                s = fma(q[d], k_tile[j * DH + d], s);
            }
            scores[j] = s;
            block_max = max(block_max, s);
        }
        let new_m = max(m, block_max);
        let rescale = exp(m - new_m);
        l *= rescale;
        for (var d = 0u; d < DH; d++) {
            o[d] *= rescale;
        }
        for (var j = 0u; j < n; j++) {
            let pj = exp(scores[j] - new_m);
            l += pj;
            for (var d = 0u; d < DH; d++) {
                o[d] = fma(pj, v_tile[j * DH + d], o[d]);
            }
        }
        m = new_m;
        workgroupBarrier();
    }

    if (valid) {
        let gate = 1.0 / (1.0 + exp(-qkvg[me * p.width + 3u * p.inner + head]));
        let scale = gate / l;
        for (var d = 0u; d < DH; d++) {
            out[me * p.inner + head * DH + d] = o[d] * scale;
        }
    }
}
