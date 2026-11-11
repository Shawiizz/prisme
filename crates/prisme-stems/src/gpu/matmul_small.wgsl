// matmul.wgsl in smaller tiles, for GPUs with fewer registers per thread than NVIDIA's
// (Intel, AMD): 64 x 64 output tiles, 16 x 16 threads each computing 4 x 4 outputs strided
// by 16. Every global access is bounds-checked here: the module is created without
// runtime checks.

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
}

@group(0) @binding(0) var<storage, read> a: array<f32>;
@group(0) @binding(1) var<storage, read> w: array<f32>;
@group(0) @binding(2) var<storage, read> bias: array<f32>;
@group(0) @binding(3) var<storage, read_write> c: array<f32>;
@group(0) @binding(4) var<uniform> p: Params;

const TILE: u32 = 64u;
const BK: u32 = 16u;
// tile rows padded so that the transposed stores do not conflict
const STRIDE: u32 = 68u;

var<workgroup> a_tile: array<f32, 1088>;
var<workgroup> w_tile: array<f32, 1088>;

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

@compute @workgroup_size(16, 16)
fn main(
    @builtin(workgroup_id) wg: vec3<u32>,
    @builtin(local_invocation_id) lid: vec3<u32>,
    @builtin(local_invocation_index) li: u32,
) {
    let row0 = wg.y * TILE;
    let col0 = wg.x * TILE;
    var acc: array<vec4<f32>, 4>;

    for (var k0 = 0u; k0 < p.k; k0 += BK) {
        for (var i = 0u; i < 4u; i++) {
            let e = li + i * 256u;
            let r = e / BK;
            let kk = e % BK;
            let gk = k0 + kk;
            var av = 0.0;
            if (row0 + r < p.m && gk < p.k) {
                av = a[p.a_off + (row0 + r) * p.lda + gk];
            }
            a_tile[kk * STRIDE + r] = av;
            var wv = 0.0;
            if (col0 + r < p.n && gk < p.k) {
                wv = w[(col0 + r) * p.k + gk];
            }
            w_tile[kk * STRIDE + r] = wv;
        }
        workgroupBarrier();
        for (var kk = 0u; kk < BK; kk++) {
            let wv = vec4(
                w_tile[kk * STRIDE + lid.x],
                w_tile[kk * STRIDE + lid.x + 16u],
                w_tile[kk * STRIDE + lid.x + 32u],
                w_tile[kk * STRIDE + lid.x + 48u],
            );
            for (var i = 0u; i < 4u; i++) {
                acc[i] = fma(vec4(a_tile[kk * STRIDE + lid.y + 16u * i]), wv, acc[i]);
            }
        }
        workgroupBarrier();
    }

    for (var i = 0u; i < 4u; i++) {
        let r = row0 + lid.y + 16u * i;
        if (r >= p.m) {
            continue;
        }
        for (var j = 0u; j < 4u; j++) {
            let col = col0 + lid.x + 16u * j;
            if (col >= p.n) {
                continue;
            }
            var v = acc[i][j];
            if (p.has_bias != 0u) {
                v += bias[col];
            }
            v = activate(v);
            let at = p.c_off + r * p.ldc + col;
            if (p.accumulate != 0u) {
                v += c[at];
            }
            c[at] = v;
        }
    }
}
