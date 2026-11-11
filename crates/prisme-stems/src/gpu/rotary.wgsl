// Rotary position embedding, in place on the queries and keys of every token: each pair
// (x[2i], x[2i+1]) of a head is rotated by its angle at the token's position in its sequence.

struct Params {
    tokens: u32,
    // query and key heads together: 2 * heads
    heads: u32,
    pairs: u32,
    width: u32,
    bands: u32,
    // 0: positions along the frames (time), 1: along the bands (frequency)
    axis: u32,
    positions: u32,
    groups_x: u32,
}

@group(0) @binding(0) var<storage, read_write> qkvg: array<Act>;
// cos for every (position, pair), then sin
@group(0) @binding(1) var<storage, read> angles: array<f32>;
@group(0) @binding(2) var<uniform> p: Params;

@compute @workgroup_size(256)
fn main(@builtin(workgroup_id) wg: vec3<u32>, @builtin(local_invocation_index) lid: u32) {
    let id = (wg.x + wg.y * p.groups_x) * 256u + lid;
    let per_token = p.heads * p.pairs;
    if (id >= p.tokens * per_token) {
        return;
    }
    let token = id / per_token;
    let rest = id % per_token;
    let pair = rest % p.pairs;
    let pos = select(token / p.bands, token % p.bands, p.axis == 1u);
    let at = token * p.width + (rest / p.pairs) * p.pairs * 2u + pair * 2u;
    let c = angles[pos * p.pairs + pair];
    let s = angles[(p.positions + pos) * p.pairs + pair];
    let a = f32(qkvg[at]);
    let b = f32(qkvg[at + 1u]);
    qkvg[at] = Act(a * c - b * s);
    qkvg[at + 1u] = Act(b * c + a * s);
}
