// c[r, j] (+)= act(sum_k a[r, k] * w[j, k] + bias[j]): a linear layer over `m` rows.
//
// 128 x 128 output tiles, 16 x 16 threads each computing 8 x 8 outputs strided by 16 (so that
// threads read the shared tiles without bank conflicts and write consecutive addresses).
// Every global access is bounds-checked here: the module is created without runtime checks.

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

const TILE: u32 = 128u;
const BK: u32 = 8u;
// tile rows padded so that the transposed stores do not conflict either
const STRIDE: u32 = 132u;

var<workgroup> a_tile: array<f32, 1056>;
var<workgroup> w_tile: array<f32, 1056>;

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
    var acc: array<array<f32, 8>, 8>;

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
            var av: array<f32, 8>;
            var wv: array<f32, 8>;
            for (var i = 0u; i < 8u; i++) {
                av[i] = a_tile[kk * STRIDE + lid.y + 16u * i];
                wv[i] = w_tile[kk * STRIDE + lid.x + 16u * i];
            }
            for (var i = 0u; i < 8u; i++) {
                for (var j = 0u; j < 8u; j++) {
                    acc[i][j] = fma(av[i], wv[j], acc[i][j]);
                }
            }
        }
        workgroupBarrier();
    }

    for (var i = 0u; i < 8u; i++) {
        let r = row0 + lid.y + 16u * i;
        if (r >= p.m) {
            continue;
        }
        for (var j = 0u; j < 8u; j++) {
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
