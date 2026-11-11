// Gated multi-head attention along one axis, in one pass over the keys (online softmax):
// a workgroup takes 64 queries of one sequence and one head, each thread one query, and
// walks the keys and values 32 at a time through shared memory. Each head's output is
// scaled by the sigmoid of its gate. The queries come pre-scaled and rotated.
//
// A thread holds its query and its output, four values to a vector, and no scores: the
// running maximum of the softmax moves only when a score passes it by SLACK, so the
// output is seldom rescaled (as in attention_coop.wgsl). Few registers, which the GPUs
// without NVIDIA's register files need.

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

@group(0) @binding(0) var<storage, read> qkvg: array<Act>;
@group(0) @binding(1) var<storage, read_write> out: array<Act>;
@group(0) @binding(2) var<uniform> p: Params;

// a multiple of 4, at most 4 * MAX_DH4
override DH: u32 = 64u;
const QUERIES: u32 = 64u;
const KEYS: u32 = 32u;
const MAX_DH4: u32 = 16u;
// exp(8) leaves the sums far from the f32 range
const SLACK: f32 = 8.0;

var<workgroup> k_tile: array<vec4<f32>, 512>;
var<workgroup> v_tile: array<vec4<f32>, 512>;

fn token(group: u32, i: u32) -> u32 {
    return select(i * p.bands + group, group * p.bands + i, p.axis == 1u);
}

fn load4(at: u32) -> vec4<f32> {
    return vec4(f32(qkvg[at]), f32(qkvg[at + 1u]), f32(qkvg[at + 2u]), f32(qkvg[at + 3u]));
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
    let dh4 = DH / 4u;

    var q: array<vec4<f32>, MAX_DH4>;
    var o: array<vec4<f32>, MAX_DH4>;
    for (var d = 0u; d < dh4; d++) {
        q[d] = load4(me * p.width + head * DH + 4u * d);
        o[d] = vec4(0.0);
    }
    var m = -3.0e38;
    var l = 0.0;

    for (var k0 = 0u; k0 < p.len; k0 += KEYS) {
        for (var e = lid; e < KEYS * dh4; e += QUERIES) {
            let key = k0 + e / dh4;
            var kv = vec4(0.0);
            var vv = vec4(0.0);
            if (key < p.len) {
                let at = token(group, key) * p.width + head * DH + 4u * (e % dh4);
                kv = load4(at + p.inner);
                vv = load4(at + 2u * p.inner);
            }
            k_tile[e] = kv;
            v_tile[e] = vv;
        }
        workgroupBarrier();
        let n = min(KEYS, p.len - k0);
        for (var j = 0u; j < n; j++) {
            var dot4 = vec4(0.0);
            for (var d = 0u; d < dh4; d++) {
                dot4 = fma(q[d], k_tile[j * dh4 + d], dot4);
            }
            let s = dot4.x + dot4.y + dot4.z + dot4.w;
            if (s > m + SLACK) {
                let rescale = exp(m - s);
                l *= rescale;
                for (var d = 0u; d < dh4; d++) {
                    o[d] *= rescale;
                }
                m = s;
            }
            let pj = exp(s - m);
            l += pj;
            for (var d = 0u; d < dh4; d++) {
                o[d] = fma(vec4(pj), v_tile[j * dh4 + d], o[d]);
            }
        }
        workgroupBarrier();
    }

    if (valid) {
        let gate = 1.0 / (1.0 + exp(-f32(qkvg[me * p.width + 3u * p.inner + head])));
        let scale = gate / l;
        for (var d = 0u; d < dh4; d++) {
            let v = o[d] * scale;
            let at = me * p.inner + head * DH + 4u * d;
            out[at] = Act(v.x);
            out[at + 1u] = Act(v.y);
            out[at + 2u] = Act(v.z);
            out[at + 3u] = Act(v.w);
        }
    }
}
