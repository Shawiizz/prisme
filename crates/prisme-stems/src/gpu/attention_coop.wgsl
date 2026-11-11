// attention.wgsl on the matrix units, for heads of 64 values, with the rotary embedding of
// the queries and keys (rotary.wgsl) done on the way in. A workgroup takes 64 queries of one
// sequence and one head, 16 per subgroup, and walks the keys 16 at a time: scores and
// weighted values are f16 products summed in f32, the softmax runs in f32 through shared
// memory, two lanes per query row.
//
// Every workgroup variable is a multiple of 16 bytes long, and every offset handed to a
// cooperative load or store a multiple of 16 bytes: the matrix units read 16-byte aligned
// tiles.

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
// the same buffer as `qkvg`, 4 values at a time
@group(0) @binding(2) var<storage, read> qkvg4: array<vec4<Act>>;
// cos for every (position, pair), then sin
@group(0) @binding(3) var<storage, read> angles: array<f32>;
@group(0) @binding(4) var<uniform> p: Params;

const DH: u32 = 64u;
const PAIRS: u32 = 32u;
const KEYS: u32 = 16u;
// row strides: the queries and the key and value tiles, the probabilities (f16), the scores
// and the output on its way out (f32)
const LD: u32 = 72u;
const LDP: u32 = 24u;
const LDS: u32 = 36u;
// how far above the reference maximum a score may go before the output is rescaled (the
// probabilities, below e^8, fit an f16 comfortably)
const SLACK: f32 = 8.0;

// the 64 rotated queries, then each subgroup's 16 rows of probabilities
var<workgroup> qp_sh: array<f16, 4608>;
var<workgroup> k_sh: array<f16, 1152>;
var<workgroup> v_sh: array<f16, 1152>;
// each subgroup's 16 x 16 scores, or 16 x 32 of its output
var<workgroup> s_sh: array<f32, 2304>;
// whether a row raised its maximum during this block
var<workgroup> rescaling: array<u32, 4>;

fn token(group: u32, i: u32) -> u32 {
    return select(i * p.bands + group, group * p.bands + i, p.axis == 1u);
}

// Values 4 q to 4 q + 3 of a query or key at `pos` in its sequence, rotated: two pairs.
fn rotate(v: vec4<Act>, pos: u32, q: u32) -> vec4<f32> {
    let pair = 2u * q;
    let c = vec2(angles[pos * PAIRS + pair], angles[pos * PAIRS + pair + 1u]);
    let s = vec2(angles[(p.len + pos) * PAIRS + pair], angles[(p.len + pos) * PAIRS + pair + 1u]);
    let x = vec2(f32(v.x), f32(v.z));
    let y = vec2(f32(v.y), f32(v.w));
    let rx = x * c - y * s;
    let ry = y * c + x * s;
    return vec4(rx.x, ry.x, rx.y, ry.y);
}

// The rotated keys and the values of the block from k0 this thread moves: 4 values from
// 4 (e % 16) of key e / 16, for e = li + 128 t.
fn fetch(
    kr: ptr<function, array<vec4<f32>, 2>>,
    vr: ptr<function, array<vec4<f32>, 2>>,
    group: u32,
    head: u32,
    k0: u32,
    li: u32,
) {
    for (var t = 0u; t < 2u; t++) {
        let e = li + 128u * t;
        let key = k0 + e / 16u;
        var k = vec4<f32>();
        var v = vec4<f32>();
        if (key < p.len) {
            let at = (token(group, key) * p.width + head * DH) / 4u + e % 16u;
            k = rotate(qkvg4[at + p.inner / 4u], key, e % 16u);
            v = vec4<f32>(qkvg4[at + p.inner / 2u]);
        }
        (*kr)[t] = k;
        (*vr)[t] = v;
    }
}

@compute @workgroup_size(128)
fn main(
    @builtin(workgroup_id) wg: vec3<u32>,
    @builtin(local_invocation_index) li: u32,
    @builtin(subgroup_id) sg: u32,
    @builtin(subgroup_invocation_id) lane: u32,
) {
    let block = wg.x + wg.y * p.groups_x;
    if (block >= p.blocks) {
        return;
    }
    let query_block = block % p.query_blocks;
    let group_head = block / p.query_blocks;
    let head = group_head % p.heads;
    let group = group_head / p.heads;
    let q0 = query_block * 64u;

    for (var t = 0u; t < 8u; t++) {
        let e = li + 128u * t;
        let r = e / 16u;
        let i = q0 + r;
        var v = vec4<f32>();
        if (i < p.len) {
            v = rotate(qkvg4[(token(group, i) * p.width + head * DH) / 4u + e % 16u], i, e % 16u);
        }
        let at = r * LD + (e % 16u) * 4u;
        qp_sh[at] = f16(v.x);
        qp_sh[at + 1u] = f16(v.y);
        qp_sh[at + 2u] = f16(v.z);
        qp_sh[at + 3u] = f16(v.w);
    }
    workgroupBarrier();
    let mine = sg * 16u * LD;
    let q_0 = coopLoadT<coop_mat16x16<f16, A>>(&qp_sh[mine], LD);
    let q_1 = coopLoadT<coop_mat16x16<f16, A>>(&qp_sh[mine + 16u], LD);
    let q_2 = coopLoadT<coop_mat16x16<f16, A>>(&qp_sh[mine + 32u], LD);
    let q_3 = coopLoadT<coop_mat16x16<f16, A>>(&qp_sh[mine + 48u], LD);
    var o_0 = coop_mat16x16<f32, C>();
    var o_1 = coop_mat16x16<f32, C>();
    var o_2 = coop_mat16x16<f32, C>();
    var o_3 = coop_mat16x16<f32, C>();

    // two lanes per query row, each taking every other column, in an order that keeps the
    // 32 lanes on distinct banks
    let row = lane % 16u;
    let half = lane / 16u;
    let skew = row / 8u;
    let s_base = sg * 16u * LDS;
    let s_row = s_base + row * LDS;
    let p_base = sg * 16u * LDP;
    let p_row = p_base + row * LDP;
    // the maximum the probabilities are relative to: raised only when a score exceeds it by
    // more than SLACK, so that the output is rarely rescaled
    var m = -3.0e38;
    var l = 0.0;

    // the next block's keys and values, loaded while this one is worked on
    var kr: array<vec4<f32>, 2>;
    var vr: array<vec4<f32>, 2>;
    fetch(&kr, &vr, group, head, 0u, li);
    for (var k0 = 0u; k0 < p.len; k0 += KEYS) {
        workgroupBarrier();
        for (var t = 0u; t < 2u; t++) {
            let e = li + 128u * t;
            let at = (e / 16u) * LD + (e % 16u) * 4u;
            let k = kr[t];
            let v = vr[t];
            k_sh[at] = f16(k.x);
            k_sh[at + 1u] = f16(k.y);
            k_sh[at + 2u] = f16(k.z);
            k_sh[at + 3u] = f16(k.w);
            v_sh[at] = f16(v.x);
            v_sh[at + 1u] = f16(v.y);
            v_sh[at + 2u] = f16(v.z);
            v_sh[at + 3u] = f16(v.w);
        }
        if (k0 + KEYS < p.len) {
            fetch(&kr, &vr, group, head, k0 + KEYS, li);
        }
        if (li == 0u) {
            rescaling[0] = 0u;
        }
        workgroupBarrier();

        var s = coop_mat16x16<f32, C>();
        s = coopMultiplyAdd(q_0, coopLoad<coop_mat16x16<f16, B>>(&k_sh[0], LD), s);
        s = coopMultiplyAdd(q_1, coopLoad<coop_mat16x16<f16, B>>(&k_sh[16], LD), s);
        s = coopMultiplyAdd(q_2, coopLoad<coop_mat16x16<f16, B>>(&k_sh[32], LD), s);
        s = coopMultiplyAdd(q_3, coopLoad<coop_mat16x16<f16, B>>(&k_sh[48], LD), s);
        coopStoreT(s, &s_sh[s_base], LDS);
        workgroupBarrier();

        let valid = min(KEYS, p.len - k0);
        var block_max = -3.0e38;
        for (var j = 0u; j < 8u; j++) {
            let c = 2u * ((j + skew) % 8u) + half;
            if (c < valid) {
                block_max = max(block_max, s_sh[s_row + c]);
            }
        }
        block_max = max(block_max, subgroupShuffleXor(block_max, 16u));
        var rescale = 1.0;
        if (block_max > m + SLACK) {
            rescale = exp(m - block_max);
            m = block_max;
            if (k0 > 0u) {
                rescaling[0] = 1u;
            }
        }
        var sum = 0.0;
        for (var j = 0u; j < 8u; j++) {
            let c = 2u * ((j + skew) % 8u) + half;
            var e = 0.0;
            if (c < valid) {
                e = exp(s_sh[s_row + c] - m);
            }
            sum += e;
            qp_sh[p_row + c] = f16(e);
        }
        sum += subgroupShuffleXor(sum, 16u);
        l = l * rescale + sum;
        if (workgroupUniformLoad(&rescaling[0]) != 0u) {
            coopStoreT(o_0, &s_sh[s_base], LDS);
            coopStoreT(o_1, &s_sh[s_base + 16u], LDS);
            workgroupBarrier();
            for (var j = 0u; j < 16u; j++) {
                s_sh[s_row + 2u * j + half] *= rescale;
            }
            workgroupBarrier();
            o_0 = coopLoadT<coop_mat16x16<f32, C>>(&s_sh[s_base], LDS);
            o_1 = coopLoadT<coop_mat16x16<f32, C>>(&s_sh[s_base + 16u], LDS);
            workgroupBarrier();
            coopStoreT(o_2, &s_sh[s_base], LDS);
            coopStoreT(o_3, &s_sh[s_base + 16u], LDS);
            workgroupBarrier();
            for (var j = 0u; j < 16u; j++) {
                s_sh[s_row + 2u * j + half] *= rescale;
            }
            workgroupBarrier();
            o_2 = coopLoadT<coop_mat16x16<f32, C>>(&s_sh[s_base], LDS);
            o_3 = coopLoadT<coop_mat16x16<f32, C>>(&s_sh[s_base + 16u], LDS);
        }

        let pa = coopLoadT<coop_mat16x16<f16, A>>(&qp_sh[p_base], LDP);
        o_0 = coopMultiplyAdd(pa, coopLoadT<coop_mat16x16<f16, B>>(&v_sh[0], LD), o_0);
        o_1 = coopMultiplyAdd(pa, coopLoadT<coop_mat16x16<f16, B>>(&v_sh[16], LD), o_1);
        o_2 = coopMultiplyAdd(pa, coopLoadT<coop_mat16x16<f16, B>>(&v_sh[32], LD), o_2);
        o_3 = coopMultiplyAdd(pa, coopLoadT<coop_mat16x16<f16, B>>(&v_sh[48], LD), o_3);
    }

    let i = q0 + sg * 16u + row;
    let me = token(group, min(i, p.len - 1u));
    let gate = 1.0 / (1.0 + exp(-f32(qkvg[me * p.width + 3u * p.inner + head])));
    let scale = gate / l;
    let to = me * p.inner + head * DH;
    workgroupBarrier();
    coopStoreT(o_0, &s_sh[s_base], LDS);
    coopStoreT(o_1, &s_sh[s_base + 16u], LDS);
    workgroupBarrier();
    if (i < p.len) {
        for (var j = 0u; j < 16u; j++) {
            let c = 2u * j + half;
            out[to + c] = Act(s_sh[s_row + c] * scale);
        }
    }
    workgroupBarrier();
    coopStoreT(o_2, &s_sh[s_base], LDS);
    coopStoreT(o_3, &s_sh[s_base + 16u], LDS);
    workgroupBarrier();
    if (i < p.len) {
        for (var j = 0u; j < 16u; j++) {
            let c = 2u * j + half;
            out[to + 32u + c] = Act(s_sh[s_row + c] * scale);
        }
    }
}
