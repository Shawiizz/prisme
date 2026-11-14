// The attention of learned queries pooling sequences of tokens: a workgroup takes one query
// of one head over one sequence, 64 keys at a time (online softmax), each thread a key, then
// a value of the head. The queries come pre-scaled, the keys unrotated; heads of queries may
// share their keys and values.

struct Params {
    len: u32,
    // tokens between the first of a sequence and the first of the next
    stride: u32,
    // width of a token's keys then values
    width: u32,
    kv_inner: u32,
    queries: u32,
    heads: u32,
    kv_share: u32,
    inner: u32,
    // sequences * queries * heads
    blocks: u32,
    groups_x: u32,
}

@group(0) @binding(0) var<storage, read> query: array<f32>;
@group(0) @binding(1) var<storage, read> kv: array<Act>;
@group(0) @binding(2) var<storage, read_write> out: array<Act>;
@group(0) @binding(3) var<uniform> p: Params;

// DH, the values of a head (at most 64), is defined before this source
const KEYS: u32 = 64u;

var<workgroup> scores: array<f32, KEYS>;

@compute @workgroup_size(64)
fn main(@builtin(workgroup_id) wg: vec3<u32>, @builtin(local_invocation_index) lid: u32) {
    let block = wg.x + wg.y * p.groups_x;
    if (block >= p.blocks) {
        return;
    }
    let head = block % p.heads;
    let q = block / p.heads % p.queries;
    let sequence = block / (p.heads * p.queries);
    let first = sequence * p.stride;
    let keys = head / p.kv_share * DH;
    let mine = q * p.inner + head * DH;

    var m = -3.0e38;
    var l = 0.0;
    var o = 0.0;
    for (var k0 = 0u; k0 < p.len; k0 += KEYS) {
        let key = k0 + lid;
        var s = -3.0e38;
        if (key < p.len) {
            let at = (first + key) * p.width + keys;
            s = 0.0;
            for (var d = 0u; d < DH; d++) {
                s = fma(query[mine + d], f32(kv[at + d]), s);
            }
        }
        scores[lid] = s;
        workgroupBarrier();
        let n = min(KEYS, p.len - k0);
        var top = m;
        for (var j = 0u; j < n; j++) {
            top = max(top, scores[j]);
        }
        let rescale = exp(m - top);
        l *= rescale;
        o *= rescale;
        m = top;
        for (var j = 0u; j < n; j++) {
            let w = exp(scores[j] - m);
            l += w;
            if (lid < DH) {
                o = fma(w, f32(kv[(first + k0 + j) * p.width + p.kv_inner + keys + lid]), o);
            }
        }
        workgroupBarrier();
    }
    if (lid < DH) {
        out[(sequence * p.queries + q) * p.inner + head * DH + lid] = Act(o / l);
    }
}
