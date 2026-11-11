// matmul.wgsl on the matrix units: c[r, j] (+)= act(sum_k a[r, k] * w[j, k] + bias[j]), the
// products in f16 (the weights are f16 already), summed in f32. `In` and `Out` are the storage
// types of `a` and `c` (f32 or f16).
//
// 128 x 128 output tiles, 8 subgroups each computing 64 x 32 outputs as 4 x 2 cooperative
// 16 x 16 matrices. The tiles of `a` and `w` go through shared memory as f16; `k` and the
// rows of `a` come in multiples of 4, `w` rows padded to `ldw` groups of 4.

struct Params {
    m: u32,
    n: u32,
    k: u32,
    lda: u32,
    a_off: u32,
    ldc: u32,
    c_off: u32,
    // 0: none, 1: GELU, 2: tanh
    act: u32,
    accumulate: u32,
    has_bias: u32,
    ldw: u32,
}

@group(0) @binding(0) var<storage, read> a: array<vec4<In>>;
@group(0) @binding(1) var<storage, read> w: array<vec4<f16>>;
@group(0) @binding(2) var<storage, read> bias: array<f32>;
@group(0) @binding(3) var<storage, read_write> c: array<Out>;
@group(0) @binding(4) var<uniform> p: Params;

const BK: u32 = 32u;
// row stride of the shared tiles, padded against bank conflicts (and a multiple of 8, as
// every offset handed to a cooperative load or store keeps 16-byte alignment)
const LDS: u32 = 40u;

var<workgroup> a_sh: array<f16, 5120>;
var<workgroup> w_sh: array<f16, 5120>;
// each subgroup's tile on its way out
var<workgroup> c_sh: array<f32, 2048>;

fn erf(x: f32) -> f32 {
    // Abramowitz & Stegun 7.1.26, |error| < 1.5e-7
    let t = 1.0 / (1.0 + 0.3275911 * abs(x));
    let y = 1.0 - ((((1.061405429 * t - 1.453152027) * t + 1.421413741) * t - 0.284496736) * t + 0.254829592) * t * exp(-x * x);
    return select(-y, y, x >= 0.0);
}

fn activate(x: f32) -> f32 {
    switch p.act {
        case 1u: { return 0.5 * x * (1.0 + erf(x * 0.70710678118654752)); }
        case 2u: { return tanh(clamp(x, -15.0, 15.0)); }
        default: { return x; }
    }
}

fn store(m: coop_mat16x16<f32, C>, sg: u32, lane: u32, row0: u32, col0: u32) {
    coopStoreT(m, &c_sh[sg * 256u], 16u);
    workgroupBarrier();
    for (var t = 0u; t < 8u; t++) {
        let e = lane + 32u * t;
        let r = row0 + e / 16u;
        let col = col0 + e % 16u;
        if (r < p.m && col < p.n) {
            var v = c_sh[sg * 256u + e];
            if (p.has_bias != 0u) {
                v += bias[col];
            }
            v = activate(v);
            let at = p.c_off + r * p.ldc + col;
            if (p.accumulate != 0u) {
                v += f32(c[at]);
            }
            c[at] = Out(v);
        }
    }
    workgroupBarrier();
}

@compute @workgroup_size(256)
fn main(
    @builtin(workgroup_id) wg: vec3<u32>,
    @builtin(local_invocation_index) li: u32,
    @builtin(subgroup_id) sg: u32,
    @builtin(subgroup_invocation_id) lane: u32,
) {
    let row0 = wg.y * 128u;
    let col0 = wg.x * 128u;
    let wm = (sg / 4u) * 64u;
    let wn = (sg % 4u) * 32u;
    var c00 = coop_mat16x16<f32, C>();
    var c01 = coop_mat16x16<f32, C>();
    var c10 = coop_mat16x16<f32, C>();
    var c11 = coop_mat16x16<f32, C>();
    var c20 = coop_mat16x16<f32, C>();
    var c21 = coop_mat16x16<f32, C>();
    var c30 = coop_mat16x16<f32, C>();
    var c31 = coop_mat16x16<f32, C>();

    for (var k0 = 0u; k0 < p.k; k0 += BK) {
        // each thread moves 4 groups of 4 values of `a` and of `w`: row e / 8, from 4 (e % 8)
        for (var i = 0u; i < 4u; i++) {
            let e = li + i * 256u;
            let r = e / 8u;
            let k = k0 + (e % 8u) * 4u;
            let at = r * LDS + (e % 8u) * 4u;
            var v = vec4<In>();
            if (row0 + r < p.m && k < p.k) {
                v = a[(p.a_off + (row0 + r) * p.lda + k) / 4u];
            }
            a_sh[at] = f16(v.x);
            a_sh[at + 1u] = f16(v.y);
            a_sh[at + 2u] = f16(v.z);
            a_sh[at + 3u] = f16(v.w);
            var u = vec4<f16>();
            if (col0 + r < p.n && k < p.k) {
                u = w[(col0 + r) * p.ldw + k / 4u];
            }
            w_sh[at] = u.x;
            w_sh[at + 1u] = u.y;
            w_sh[at + 2u] = u.z;
            w_sh[at + 3u] = u.w;
        }
        workgroupBarrier();
        for (var kk = 0u; kk < BK; kk += 16u) {
            let a0 = coopLoadT<coop_mat16x16<f16, A>>(&a_sh[wm * LDS + kk], LDS);
            let a1 = coopLoadT<coop_mat16x16<f16, A>>(&a_sh[(wm + 16u) * LDS + kk], LDS);
            let a2 = coopLoadT<coop_mat16x16<f16, A>>(&a_sh[(wm + 32u) * LDS + kk], LDS);
            let a3 = coopLoadT<coop_mat16x16<f16, A>>(&a_sh[(wm + 48u) * LDS + kk], LDS);
            let b0 = coopLoad<coop_mat16x16<f16, B>>(&w_sh[wn * LDS + kk], LDS);
            let b1 = coopLoad<coop_mat16x16<f16, B>>(&w_sh[(wn + 16u) * LDS + kk], LDS);
            c00 = coopMultiplyAdd(a0, b0, c00);
            c01 = coopMultiplyAdd(a0, b1, c01);
            c10 = coopMultiplyAdd(a1, b0, c10);
            c11 = coopMultiplyAdd(a1, b1, c11);
            c20 = coopMultiplyAdd(a2, b0, c20);
            c21 = coopMultiplyAdd(a2, b1, c21);
            c30 = coopMultiplyAdd(a3, b0, c30);
            c31 = coopMultiplyAdd(a3, b1, c31);
        }
        workgroupBarrier();
    }

    let r = row0 + wm;
    let col = col0 + wn;
    store(c00, sg, lane, r, col);
    store(c01, sg, lane, r, col + 16u);
    store(c10, sg, lane, r + 16u, col);
    store(c11, sg, lane, r + 16u, col + 16u);
    store(c20, sg, lane, r + 32u, col);
    store(c21, sg, lane, r + 32u, col + 16u);
    store(c30, sg, lane, r + 48u, col);
    store(c31, sg, lane, r + 48u, col + 16u);
}
