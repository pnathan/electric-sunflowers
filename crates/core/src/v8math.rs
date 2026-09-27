//! Port of v8/src/base/ieee754.cc (fdlibm, Sun Microsystems 1993, modified by
//! Google 2016, fetched from the v8 repo at tag 13.6.233 to match node 24's
//! V8). This exists to make sfcore::js bit-exact with V8's Math.sin/cos/log/
//! log2/log10: V8 uses this fdlibm code by default (glibc trig is used only
//! in a "component build" of V8, which node does not ship).
//!
//! JS parity: every branch, constant and operation order below matches the
//! C++ source line for line. Do not simplify the algorithm; the exact order
//! of floating point operations is the point of this file.

#[inline]
fn get_high_word(d: f64) -> i32 {
    (d.to_bits() >> 32) as u32 as i32
}
#[inline]
fn set_high_word(d: f64, v: i32) -> f64 {
    let mut bits = d.to_bits();
    bits &= 0x0000_0000_FFFF_FFFF;
    bits |= (v as u32 as u64) << 32;
    f64::from_bits(bits)
}
#[inline]
fn set_low_word(d: f64, v: u32) -> f64 {
    let mut bits = d.to_bits();
    bits &= 0xFFFF_FFFF_0000_0000;
    bits |= v as u64;
    f64::from_bits(bits)
}
#[inline]
fn insert_words(hi: i32, lo: u32) -> f64 {
    f64::from_bits(((hi as u32 as u64) << 32) | (lo as u64))
}
#[inline]
fn extract_words(d: f64) -> (i32, u32) {
    let bits = d.to_bits();
    ((bits >> 32) as u32 as i32, (bits & 0xFFFF_FFFF) as u32)
}
#[inline]
fn scalbn(x: f64, n: i32) -> f64 {
    libm::scalbn(x, n)
}

const TWO_OVER_PI: [u32; 66] = [
    0xA2F983, 0x6E4E44, 0x1529FC, 0x2757D1, 0xF534DD, 0xC0DB62, 0x95993C, 0x439041, 0xFE5163,
    0xABDEBB, 0xC561B7, 0x246E3A, 0x424DD2, 0xE00649, 0x2EEA09, 0xD1921C, 0xFE1DEB, 0x1CB129,
    0xA73EE8, 0x8235F5, 0x2EBB44, 0x84E99C, 0x7026B4, 0x5F7E41, 0x3991D6, 0x398353, 0x39F49C,
    0x845F8B, 0xBDF928, 0x3B1FF8, 0x97FFDE, 0x05980F, 0xEF2F11, 0x8B5A0A, 0x6D1F6D, 0x367ECF,
    0x27CB09, 0xB74F46, 0x3F669E, 0x5FEA2D, 0x7527BA, 0xC7EBE5, 0xF17B3D, 0x0739F7, 0x8A5292,
    0xEA6BFB, 0x5FB11F, 0x8D5D08, 0x560330, 0x46FC7B, 0x6BABF0, 0xCFBC20, 0x9AF436, 0x1DA9E3,
    0x91615E, 0xE61B08, 0x659985, 0x5F14A0, 0x68408D, 0xFFD880, 0x4D7327, 0x310606, 0x1556CA,
    0x73A8C9, 0x60E27B, 0xC08C6B,
];

const NPIO2_HW: [u32; 32] = [
    0x3FF921FB, 0x400921FB, 0x4012D97C, 0x401921FB, 0x401F6A7A, 0x4022D97C, 0x4025FDBB,
    0x402921FB, 0x402C463A, 0x402F6A7A, 0x4031475C, 0x4032D97C, 0x40346B9C, 0x4035FDBB,
    0x40378FDB, 0x403921FB, 0x403AB41B, 0x403C463A, 0x403DD85A, 0x403F6A7A, 0x40407E4C,
    0x4041475C, 0x4042106C, 0x4042D97C, 0x4043A28C, 0x40446B9C, 0x404534AC, 0x4045FDBB,
    0x4046C6CB, 0x40478FDB, 0x404858EB, 0x404921FB,
];

const PIO2_1: f64 = 1.57079632673412561417e+00;
const PIO2_1T: f64 = 6.07710050650619224932e-11;
const PIO2_2: f64 = 6.07710050630396597660e-11;
const PIO2_2T: f64 = 2.02226624879595063154e-21;
const PIO2_3: f64 = 2.02226624871116645580e-21;
const PIO2_3T: f64 = 8.47842766036889956997e-32;
const INVPIO2: f64 = 6.36619772367581382433e-01;
const TWO24: f64 = 1.67772160000000000000e+07;
const HALF_C: f64 = 5.00000000000000000000e-01;

const PIO2: [f64; 8] = [
    1.57079625129699707031e+00,
    7.54978941586159635335e-08,
    5.39030252995776476554e-15,
    3.28200341580791294123e-22,
    1.27065575308067607349e-29,
    1.22933308981111328932e-36,
    2.73370053816464559624e-44,
    2.16741683877804819444e-51,
];
const INIT_JK: [i32; 4] = [2, 3, 4, 6];
const TWON24: f64 = 5.96046447753906250000e-08;

/// __kernel_rem_pio2 from ieee754.cc.
fn kernel_rem_pio2(x: &mut [f64], y: &mut [f64], e0: i32, nx0: i32, prec: i32) -> i32 {
    let jk = INIT_JK[prec as usize];
    let jp = jk;
    let jx = nx0 - 1;
    let mut jv = (e0 - 3) / 24;
    if jv < 0 {
        jv = 0;
    }
    let mut q0 = e0 - 24 * (jv + 1);

    let mut iq = [0i32; 20];
    let mut f = [0f64; 20];
    let mut fq = [0f64; 20];
    let mut q = [0f64; 20];

    let j0 = jv - jx;
    let m = jx + jk;
    {
        let mut jj = j0;
        for i in 0..=m {
            f[i as usize] = if jj < 0 {
                0.0
            } else {
                TWO_OVER_PI[jj as usize] as i32 as f64
            };
            jj += 1;
        }
    }

    for i in 0..=jk {
        let mut fw = 0.0;
        for jj in 0..=jx {
            fw += x[jj as usize] * f[(jx + i - jj) as usize];
        }
        q[i as usize] = fw;
    }

    let mut jz = jk;
    let mut n: i32;
    let mut ih: i32;
    loop {
        let mut z;
        {
            let mut i = 0i32;
            let mut jj = jz;
            z = q[jz as usize];
            while jj > 0 {
                let fw = (TWON24 * z) as i32 as f64;
                iq[i as usize] = (z - TWO24 * fw) as i32;
                z = q[(jj - 1) as usize] + fw;
                i += 1;
                jj -= 1;
            }
        }

        z = scalbn(z, q0);
        z -= 8.0 * (z * 0.125).floor();
        n = z as i32;
        z -= n as f64;
        ih = 0;
        if q0 > 0 {
            let i = iq[(jz - 1) as usize] >> (24 - q0);
            n += i;
            iq[(jz - 1) as usize] -= i << (24 - q0);
            ih = iq[(jz - 1) as usize] >> (23 - q0);
        } else if q0 == 0 {
            ih = iq[(jz - 1) as usize] >> 23;
        } else if z >= 0.5 {
            ih = 2;
        }

        if ih > 0 {
            n += 1;
            let mut carry = 0i32;
            for i in 0..jz {
                let jj = iq[i as usize];
                if carry == 0 {
                    if jj != 0 {
                        carry = 1;
                        iq[i as usize] = 0x1000000 - jj;
                    }
                } else {
                    iq[i as usize] = 0xFFFFFF - jj;
                }
            }
            if q0 > 0 {
                match q0 {
                    1 => iq[(jz - 1) as usize] &= 0x7FFFFF,
                    2 => iq[(jz - 1) as usize] &= 0x3FFFFF,
                    _ => {}
                }
            }
            if ih == 2 {
                z = 1.0 - z;
                if carry != 0 {
                    z -= scalbn(1.0, q0);
                }
            }
        }

        if z == 0.0 {
            let mut jj = 0i32;
            for i in (jk..jz).rev() {
                jj |= iq[i as usize];
            }
            if jj == 0 {
                let mut k = 1i32;
                while jk >= k && iq[(jk - k) as usize] == 0 {
                    k += 1;
                }
                for i in (jz + 1)..=(jz + k) {
                    f[(jx + i) as usize] = TWO_OVER_PI[(jv + i) as usize] as i32 as f64;
                    let mut fw = 0.0;
                    for jj2 in 0..=jx {
                        fw += x[jj2 as usize] * f[(jx + i - jj2) as usize];
                    }
                    q[i as usize] = fw;
                }
                jz += k;
                continue;
            }
        }
        // chop off zero terms / break z into 24-bit chunk
        if z == 0.0 {
            jz -= 1;
            q0 -= 24;
            while iq[jz as usize] == 0 {
                jz -= 1;
                q0 -= 24;
            }
        } else {
            let z2 = scalbn(z, -q0);
            if z2 >= TWO24 {
                let fw = (TWON24 * z2) as i32 as f64;
                iq[jz as usize] = (z2 - TWO24 * fw) as i32;
                jz += 1;
                q0 += 24;
                iq[jz as usize] = fw as i32;
            } else {
                iq[jz as usize] = z2 as i32;
            }
        }
        break;
    }

    let mut fw = scalbn(1.0, q0);
    for i in (0..=jz).rev() {
        q[i as usize] = fw * (iq[i as usize] as f64);
        fw *= TWON24;
    }

    for i in (0..=jz).rev() {
        let mut fwk = 0.0;
        let mut k = 0i32;
        while k <= jp && k <= jz - i {
            fwk += PIO2[k as usize] * q[(i + k) as usize];
            k += 1;
        }
        fq[(jz - i) as usize] = fwk;
    }

    match prec {
        0 => {
            let mut fw2 = 0.0;
            for i in (0..=jz).rev() {
                fw2 += fq[i as usize];
            }
            y[0] = if ih == 0 { fw2 } else { -fw2 };
        }
        1 | 2 => {
            let mut fw2 = 0.0;
            for i in (0..=jz).rev() {
                fw2 += fq[i as usize];
            }
            y[0] = if ih == 0 { fw2 } else { -fw2 };
            let mut fw3 = fq[0] - fw2;
            for i in 1..=jz {
                fw3 += fq[i as usize];
            }
            y[1] = if ih == 0 { fw3 } else { -fw3 };
        }
        _ => unreachable!("prec 3 (quad) is not used by ieee754.cc callers here"),
    }
    n & 7
}

/// __ieee754_rem_pio2: remainder of x mod pi/2, returned in y[0]+y[1].
fn rem_pio2(x: f64, y: &mut [f64; 2]) -> i32 {
    let hx = get_high_word(x);
    let ix = hx & 0x7FFFFFFF;
    if ix <= 0x3FE921FB {
        y[0] = x;
        y[1] = 0.0;
        return 0;
    }
    if ix < 0x4002D97C {
        if hx > 0 {
            let mut z = x - PIO2_1;
            if ix != 0x3FF921FB {
                y[0] = z - PIO2_1T;
                y[1] = (z - y[0]) - PIO2_1T;
            } else {
                z -= PIO2_2;
                y[0] = z - PIO2_2T;
                y[1] = (z - y[0]) - PIO2_2T;
            }
            return 1;
        } else {
            let mut z = x + PIO2_1;
            if ix != 0x3FF921FB {
                y[0] = z + PIO2_1T;
                y[1] = (z - y[0]) + PIO2_1T;
            } else {
                z += PIO2_2;
                y[0] = z + PIO2_2T;
                y[1] = (z - y[0]) + PIO2_2T;
            }
            return -1;
        }
    }
    if ix <= 0x413921FB {
        let t = x.abs();
        let n = (t * INVPIO2 + HALF_C) as i32;
        let fn_ = n as f64;
        let mut r = t - fn_ * PIO2_1;
        let mut w = fn_ * PIO2_1T;
        if n < 32 && ix as u32 != NPIO2_HW[(n - 1) as usize] {
            y[0] = r - w;
        } else {
            let j = ix >> 20;
            y[0] = r - w;
            let high = get_high_word(y[0]);
            let mut i = j - ((high >> 20) & 0x7FF);
            if i > 16 {
                let t2 = r;
                w = fn_ * PIO2_2;
                r = t2 - w;
                w = fn_ * PIO2_2T - ((t2 - r) - w);
                y[0] = r - w;
                let high2 = get_high_word(y[0]);
                i = j - ((high2 >> 20) & 0x7FF);
                if i > 49 {
                    let t3 = r;
                    w = fn_ * PIO2_3;
                    r = t3 - w;
                    w = fn_ * PIO2_3T - ((t3 - r) - w);
                    y[0] = r - w;
                }
            }
        }
        y[1] = (r - y[0]) - w;
        return if hx < 0 {
            y[0] = -y[0];
            y[1] = -y[1];
            -n
        } else {
            n
        };
    }
    if ix >= 0x7FF00000 {
        y[0] = x - x;
        y[1] = x - x;
        return 0;
    }
    let low = extract_words(x).1;
    let mut z = set_low_word(0.0, low);
    let e0 = (ix >> 20) - 1046;
    z = set_high_word(z, ix - ((e0 as u32) << 20) as i32);
    let mut tx = [0f64; 3];
    for i in 0..2 {
        tx[i] = (z as i32) as f64;
        z = (z - tx[i]) * TWO24;
    }
    tx[2] = z;
    let mut nx = 3i32;
    while tx[(nx - 1) as usize] == 0.0 {
        nx -= 1;
    }
    let n = kernel_rem_pio2(&mut tx[..nx as usize], y, e0, nx, 2);
    if hx < 0 {
        y[0] = -y[0];
        y[1] = -y[1];
        -n
    } else {
        n
    }
}

const KC1: f64 = 4.16666666666666019037e-02;
const KC2: f64 = -1.38888888888741095749e-03;
const KC3: f64 = 2.48015872894767294178e-05;
const KC4: f64 = -2.75573143513906633035e-07;
const KC5: f64 = 2.08757232129817482790e-09;
const KC6: f64 = -1.13596475577881948265e-11;

/// __kernel_cos on [-pi/4, pi/4].
fn kernel_cos(x: f64, y: f64) -> f64 {
    let mut ix = get_high_word(x);
    ix &= 0x7FFFFFFF;
    if ix < 0x3E400000 {
        if (x as i32) == 0 {
            return 1.0;
        }
    }
    let z = x * x;
    let r = z * (KC1 + z * (KC2 + z * (KC3 + z * (KC4 + z * (KC5 + z * KC6)))));
    if ix < 0x3FD33333 {
        1.0 - (0.5 * z - (z * r - x * y))
    } else {
        let qx = if ix > 0x3FE90000 {
            0.28125
        } else {
            insert_words(ix - 0x00200000, 0)
        };
        let iz = 0.5 * z - qx;
        let a = 1.0 - qx;
        a - (iz - (z * r - x * y))
    }
}

const KS1: f64 = -1.66666666666666324348e-01;
const KS2: f64 = 8.33333333332248946124e-03;
const KS3: f64 = -1.98412698298579493134e-04;
const KS4: f64 = 2.75573137070700676789e-06;
const KS5: f64 = -2.50507602534068634195e-08;
const KS6: f64 = 1.58969099521155010221e-10;

/// __kernel_sin on [-pi/4, pi/4].
fn kernel_sin(x: f64, y: f64, iy: i32) -> f64 {
    let mut ix = get_high_word(x);
    ix &= 0x7FFFFFFF;
    if ix < 0x3E400000 {
        if (x as i32) == 0 {
            return x;
        }
    }
    let z = x * x;
    let v = z * x;
    let r = KS2 + z * (KS3 + z * (KS4 + z * (KS5 + z * KS6)));
    if iy == 0 {
        x + v * (KS1 + z * r)
    } else {
        x - ((z * (HALF_C * y - v * r) - y) - v * KS1)
    }
}

/// V8's `sin`, i.e. fdlibm_sin in ieee754.cc.
pub fn sin(x: f64) -> f64 {
    let ix = get_high_word(x) & 0x7FFFFFFF;
    if ix <= 0x3FE921FB {
        return kernel_sin(x, 0.0, 0);
    } else if ix >= 0x7FF00000 {
        return x - x;
    }
    let mut y = [0f64; 2];
    let n = rem_pio2(x, &mut y);
    match n & 3 {
        0 => kernel_sin(y[0], y[1], 1),
        1 => kernel_cos(y[0], y[1]),
        2 => -kernel_sin(y[0], y[1], 1),
        _ => -kernel_cos(y[0], y[1]),
    }
}

/// V8's `cos`, i.e. fdlibm_cos in ieee754.cc.
pub fn cos(x: f64) -> f64 {
    let ix = get_high_word(x) & 0x7FFFFFFF;
    if ix <= 0x3FE921FB {
        return kernel_cos(x, 0.0);
    } else if ix >= 0x7FF00000 {
        return x - x;
    }
    let mut y = [0f64; 2];
    let n = rem_pio2(x, &mut y);
    match n & 3 {
        0 => kernel_cos(y[0], y[1]),
        1 => -kernel_sin(y[0], y[1], 1),
        2 => -kernel_cos(y[0], y[1]),
        _ => kernel_sin(y[0], y[1], 1),
    }
}

const LN2_HI: f64 = 6.93147180369123816490e-01;
const LN2_LO: f64 = 1.90821492927058770002e-10;
const TWO54: f64 = 1.80143985094819840000e+16;
const LG1: f64 = 6.666666666666735130e-01;
const LG2: f64 = 3.999999999940941908e-01;
const LG3: f64 = 2.857142874366239149e-01;
const LG4: f64 = 2.222219843214978396e-01;
const LG5: f64 = 1.818357216161805012e-01;
const LG6: f64 = 1.531383769920937332e-01;
const LG7: f64 = 1.479819860511658591e-01;

/// V8's `log`, i.e. ieee754.cc `log(x)` (fdlibm __ieee754_log).
pub fn log(x0: f64) -> f64 {
    let mut x = x0;
    let (mut hx, mut lx) = extract_words(x);
    let mut k = 0i32;
    if hx < 0x00100000 {
        if ((hx & 0x7FFFFFFF) as u32 | lx) == 0 {
            return f64::NEG_INFINITY;
        }
        if hx < 0 {
            return f64::NAN;
        }
        k -= 54;
        x *= TWO54;
        hx = get_high_word(x);
        lx = extract_words(x).1;
    }
    if hx >= 0x7FF00000 {
        return x + x;
    }
    k += (hx >> 20) - 1023;
    hx &= 0x000FFFFF;
    let i = (hx + 0x95F64) & 0x100000;
    x = insert_words(hx | (i ^ 0x3FF00000), lx);
    k += i >> 20;
    let f = x - 1.0;
    if (0x000FFFFF & (2 + hx)) < 3 {
        if f == 0.0 {
            return if k == 0 {
                0.0
            } else {
                let dk = k as f64;
                dk * LN2_HI + dk * LN2_LO
            };
        }
        let r = f * f * (0.5 - 0.33333333333333333 * f);
        return if k == 0 {
            f - r
        } else {
            let dk = k as f64;
            dk * LN2_HI - ((r - dk * LN2_LO) - f)
        };
    }
    let s = f / (2.0 + f);
    let dk = k as f64;
    let z = s * s;
    let mut i2 = hx - 0x6147A;
    let w = z * z;
    let j2 = 0x6B851 - hx;
    let t1 = w * (LG2 + w * (LG4 + w * LG6));
    let t2 = z * (LG1 + w * (LG3 + w * (LG5 + w * LG7)));
    i2 |= j2;
    let r = t2 + t1;
    if i2 > 0 {
        let hfsq = 0.5 * f * f;
        if k == 0 {
            f - (hfsq - s * (hfsq + r))
        } else {
            dk * LN2_HI - ((hfsq - (s * (hfsq + r) + dk * LN2_LO)) - f)
        }
    } else if k == 0 {
        f - s * (f - r)
    } else {
        dk * LN2_HI - ((s * (f - r) - dk * LN2_LO) - f)
    }
}

/// k_log1p from ieee754.cc, inlined helper used by log2.
fn k_log1p(f: f64) -> f64 {
    let s = f / (2.0 + f);
    let z = s * s;
    let w = z * z;
    let t1 = w * (LG2 + w * (LG4 + w * LG6));
    let t2 = z * (LG1 + w * (LG3 + w * (LG5 + w * LG7)));
    let r = t2 + t1;
    let hfsq = 0.5 * f * f;
    s * (hfsq + r)
}

const IVLN2HI: f64 = 1.44269504072144627571e+00;
const IVLN2LO: f64 = 1.67517131648865118353e-10;

/// V8's `log2`, i.e. ieee754.cc `log2(x)`.
pub fn log2(x0: f64) -> f64 {
    let mut x = x0;
    let (mut hx, mut lx) = extract_words(x);
    let mut k = 0i32;
    if hx < 0x00100000 {
        if ((hx & 0x7FFFFFFF) as u32 | lx) == 0 {
            return f64::NEG_INFINITY;
        }
        if hx < 0 {
            return f64::NAN;
        }
        k -= 54;
        x *= TWO54;
        hx = get_high_word(x);
        lx = extract_words(x).1;
    }
    if hx >= 0x7FF00000 {
        return x + x;
    }
    if hx == 0x3FF00000 && lx == 0 {
        return 0.0;
    }
    k += (hx >> 20) - 1023;
    hx &= 0x000FFFFF;
    let i = (hx + 0x95F64) & 0x100000;
    x = insert_words(hx | (i ^ 0x3FF00000), lx);
    k += i >> 20;
    let y = k as f64;
    let f = x - 1.0;
    let hfsq = 0.5 * f * f;
    let r = k_log1p(f);

    let mut hi = f - hfsq;
    hi = set_low_word(hi, 0);
    let lo = (f - hi) - hfsq + r;
    let val_hi = hi * IVLN2HI;
    let mut val_lo = (lo + hi) * IVLN2LO + lo * IVLN2HI;

    let w = y + val_hi;
    val_lo += (y - w) + val_hi;
    let val_hi2 = w;

    val_lo + val_hi2
}

const IVLN10: f64 = 4.34294481903251816668e-01;
const LOG10_2HI: f64 = 3.01029995663611771306e-01;
const LOG10_2LO: f64 = 3.69423907715893078616e-13;

/// V8's `log10`, i.e. ieee754.cc `log10(x)`.
pub fn log10(x0: f64) -> f64 {
    let mut x = x0;
    let (mut hx, mut lx) = extract_words(x);
    let mut k = 0i32;
    if hx < 0x00100000 {
        if ((hx & 0x7FFFFFFF) as u32 | lx) == 0 {
            return f64::NEG_INFINITY;
        }
        if hx < 0 {
            return f64::NAN;
        }
        k -= 54;
        x *= TWO54;
        let e = extract_words(x);
        hx = e.0;
        lx = e.1;
    }
    if hx >= 0x7FF00000 {
        return x + x;
    }
    if hx == 0x3FF00000 && lx == 0 {
        return 0.0;
    }
    k += (hx >> 20) - 1023;

    let i = ((k as u32) & 0x80000000) >> 31;
    hx = (hx & 0x000FFFFF) | (((0x3FF - i as i32) as u32) << 20) as i32;
    let y = (k as f64) + (i as f64);
    x = set_high_word(0.0, hx);
    x = set_low_word(x, lx);

    let z = y * LOG10_2LO + IVLN10 * log(x);
    z + y * LOG10_2HI
}
