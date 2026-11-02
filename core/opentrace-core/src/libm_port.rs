//! Bit-exact Rust port of glibc's `exp`, `log` and `pow` as they run on x86-64 CPUs with FMA.
//!
//! glibc ≥ 2.28 takes these three functions from ARM optimized-routines (MIT OR Apache-2.0 WITH LLVM-exception) and,
//! on CPUs with FMA (every x86-64 since Haswell/Piledriver), dispatches to a copy compiled with `-mfma`, where GCC
//! fuses multiply-adds. The golden corpora of OpenTrace were produced with that variant. Windows' UCRT, and glibc
//! on CPUs without FMA, give different last bits for some arguments.
//!
//! This port reproduces the FMA variant operation by operation. The order of operations and every fused
//! multiply-add were taken from GCC's optimised intermediate representation (`-fdump-tree-widening_mul`, GCC 13,
//! `-O2 -mfma -ffp-contract=fast`), so each `.FMA (a, b, c)` there is `a.mul_add(b, c)` here. `mul_add` is a single
//! rounding on every platform (hardware FMA or the C library's correctly rounded `fma`), hence identical results on
//! Linux and Windows, with or without FMA hardware. Verification: `tests/libm_port.rs` and docs/desktop/BASELINE_AUDIT.md.
//!
//! Errno and floating-point exception flags are not reproduced (the analysis never reads them).
use crate::libm_port_data::*;

#[inline(always)]
fn fma(a: f64, b: f64, c: f64) -> f64 {
    a.mul_add(b, c)
}

#[inline(always)]
fn asdouble(u: u64) -> f64 {
    f64::from_bits(u)
}

const SIGN: u64 = 0x8000_0000_0000_0000;

/// `__math_oflow(sign)`: ±0x1p769 * 0x1p769.
fn oflow(sign: u32) -> f64 {
    let y = f64::from_bits(0x7000_0000_0000_0000);
    (if sign != 0 { -y } else { y }) * y
}

/// `__math_uflow(sign)`: ±0x1p-767 * 0x1p-767.
fn uflow(sign: u32) -> f64 {
    let y = f64::from_bits(0x1000_0000_0000_0000); // 0x1p-767
    (if sign != 0 { -y } else { y }) * y
}

fn invalid(x: f64) -> f64 {
    (x - x) / (x - x)
}

// ------------------------------------------------------------------------------------------------------------ exp

/// `exp`'s and `pow`'s shared tail: scale*(1+tmp) near overflow/underflow (`specialcase` of exp.c / pow.c).
#[inline(always)]
fn exp_specialcase(tmp: f64, mut sbits: u64, ki: u64, signed_result: bool) -> f64 {
    if ki & 0x8000_0000 == 0 {
        sbits = sbits.wrapping_sub(1009u64 << 52);
        let scale = asdouble(sbits);
        let y = fma(tmp, scale, scale);
        return y * f64::from_bits(0x7f00_0000_0000_0000);
    }
    sbits = sbits.wrapping_add(1022u64 << 52);
    let scale = asdouble(sbits);
    let st = tmp * scale;
    let mut y = scale + st;
    let small = if signed_result { y.abs() < 1.0 } else { y < 1.0 };
    if small {
        let one = if signed_result && y < 0.0 { -1.0 } else { 1.0 };
        let lo0 = st + (scale - y);
        let hi = y + one;
        let lo = lo0 + (y + (one - hi));
        y = (hi + lo) - one;
        if y == 0.0 {
            y = if signed_result { asdouble(sbits & SIGN) } else { 0.0 };
        }
    }
    y * f64::from_bits(0x0010_0000_0000_0000) // 0x1p-1022
}

/// glibc `exp` (FMA variant).
pub fn exp(x: f64) -> f64 {
    let ix = x.to_bits();
    let mut abstop = ((ix >> 52) & 0x7ff) as u32;
    // top12(0x1p-54) = 0x3c9, top12(512) = 0x408, top12(1024) = 0x409
    if abstop.wrapping_sub(0x3c9) >= 0x408 - 0x3c9 {
        if (abstop.wrapping_sub(0x3c9) as i32) < 0 {
            return 1.0 + x;
        }
        if abstop >= 0x409 {
            if ix == f64::NEG_INFINITY.to_bits() {
                return 0.0;
            }
            if abstop >= 0x7ff {
                return 1.0 + x;
            }
            return if ix >> 63 != 0 { uflow(0) } else { oflow(0) };
        }
        abstop = 0;
    }
    let zs = fma(x, EXP_INVLN2N, EXP_SHIFT);
    let ki = zs.to_bits();
    let kd = zs - EXP_SHIFT;
    let r = fma(kd, EXP_NEGLN2LON, fma(kd, EXP_NEGLN2HIN, x));
    let idx = 2 * (ki & 127) as usize;
    let top = ki << 45;
    let tail = asdouble(EXP_TAB[idx]);
    let sbits = top.wrapping_add(EXP_TAB[idx + 1]);
    let r2 = r * r;
    let a = r + tail;
    let b = fma(r2, fma(r, EXP_POLY[1], EXP_POLY[0]), a);
    let tmp = fma(r2 * r2, fma(r, EXP_POLY[3], EXP_POLY[2]), b);
    if abstop == 0 {
        return exp_specialcase(tmp, sbits, ki, false);
    }
    let scale = asdouble(sbits);
    fma(tmp, scale, scale)
}

// ------------------------------------------------------------------------------------------------------------ log

/// glibc `log` (FMA variant).
pub fn log(x: f64) -> f64 {
    let mut ix = x.to_bits();
    let top = (ix >> 48) as u32;
    const LO: u64 = 0x3fee_0000_0000_0000; // asuint64(1.0 - 0x1p-4)
    const HI: u64 = 0x3ff1_0900_0000_0000; // asuint64(1.0 + 0x1.09p-4)
    if ix.wrapping_sub(LO) < HI - LO {
        if ix == 1.0f64.to_bits() {
            return 0.0;
        }
        let b = &LOG_POLY1;
        let r = x - 1.0;
        let r2 = r * r;
        let r3 = r * r2;
        let p14 = fma(b[3], r2, fma(b[2], r, b[1]));
        let p21 = fma(b[6], r2, fma(b[5], r, b[4]));
        let p28 = fma(b[9], r2, fma(b[8], r, b[7]));
        let p31 = fma(b[10], r3, p28);
        let p33 = fma(p31, r3, p21);
        let p35 = fma(p33, r3, p14);
        let w36 = fma(r, f64::from_bits(0x41a0_0000_0000_0000), r);
        let rhi = fma(-r, f64::from_bits(0x41a0_0000_0000_0000), w36);
        let rlo = r - rhi;
        let rh2 = rhi * rhi;
        let hi = fma(rh2, b[0], r);
        let lo = fma(rh2, b[0], r - hi);
        let lo = fma(b[0] * rlo, r + rhi, lo);
        let y = fma(p35, r3, lo);
        return hi + y;
    }
    if top.wrapping_sub(0x0010) >= 0x7ff0 - 0x0010 {
        if ix.wrapping_mul(2) == 0 {
            return -1.0 / 0.0; // __math_divzero(1)
        }
        if ix == f64::INFINITY.to_bits() {
            return x;
        }
        if (top & 0x8000) != 0 || (top & 0x7ff0) == 0x7ff0 {
            return invalid(x);
        }
        ix = (x * f64::from_bits(0x4330_0000_0000_0000)).to_bits();
        ix = ix.wrapping_sub(52u64 << 52);
    }
    const OFF: u64 = 0x3fe6_0000_0000_0000;
    let tmp = ix.wrapping_sub(OFF);
    let i = ((tmp >> 45) & 127) as usize;
    let k = (tmp as i64) >> 52;
    let iz = ix.wrapping_sub(tmp & (0xfffu64 << 52));
    let (invc, logc) = (asdouble(LOG_TAB[i].0), asdouble(LOG_TAB[i].1));
    let z = asdouble(iz);
    let r = fma(z, invc, -1.0);
    let kd = k as f64;
    let w = fma(LOG_LN2HI, kd, logc);
    let hi = r + w;
    let lo = fma(LOG_LN2LO, kd, (w - hi) + r);
    let r2 = r * r;
    let a = &LOG_POLY;
    let p64 = fma(a[0], r2, lo);
    let p65 = r * r2;
    let p75 = fma(fma(a[4], r, a[3]), r2, fma(a[2], r, a[1]));
    fma(p65, p75, p64) + hi
}

// ------------------------------------------------------------------------------------------------------------ pow

const SIGN_BIAS: u32 = 0x800 << 7;

#[inline(always)]
fn zeroinfnan(i: u64) -> bool {
    i.wrapping_mul(2).wrapping_sub(1) >= (f64::INFINITY.to_bits() * 2).wrapping_sub(1)
}

/// 0: not an integer, 1: odd integer, 2: even integer.
fn checkint(iy: u64) -> i32 {
    let e = ((iy >> 52) & 0x7ff) as i32;
    if e < 0x3ff {
        return 0;
    }
    if e > 0x3ff + 52 {
        return 2;
    }
    let s = (0x3ff + 52 - e) as u32;
    if iy & ((1u64 << s) - 1) != 0 {
        return 0;
    }
    if iy & (1u64 << s) != 0 {
        return 1;
    }
    2
}

/// `log_inline` of pow.c: (hi, tail) with log(x) ~= hi + tail.
#[inline(always)]
fn pow_log(ix: u64) -> (f64, f64) {
    const OFF: u64 = 0x3fe6_9555_0000_0000;
    let tmp = ix.wrapping_sub(OFF);
    let i = ((tmp >> 45) & 127) as usize;
    let k = (tmp as i64) >> 52;
    let iz = ix.wrapping_sub(tmp & (0xfffu64 << 52));
    let z = asdouble(iz);
    let kd = k as f64;
    let (invc, logc, logctail) = (asdouble(POW_TAB[i].0), asdouble(POW_TAB[i].1), asdouble(POW_TAB[i].2));
    let r = fma(z, invc, -1.0);
    let t1 = fma(kd, POW_LN2HI, logc);
    let t2 = r + t1;
    let lo1 = fma(kd, POW_LN2LO, logctail);
    let lo2 = r + (t1 - t2);
    let a = &POW_POLY;
    let ar = r * a[0];
    let ar2 = r * ar;
    let ar3 = r * ar2;
    let hi = t2 + ar2;
    let lo3 = fma(r, ar, -ar2);
    let lo4 = ar2 + (t2 - hi);
    let p217 = fma(r, a[2], a[1]);
    let p221 = fma(r, a[4], a[3]);
    let p225 = fma(r, a[6], a[5]);
    let p229 = fma(ar2, fma(ar2, p225, p221), p217);
    let lo = fma(ar3, p229, lo4 + (lo3 + (lo1 + lo2)));
    let y = hi + lo;
    (y, lo + (hi - y))
}

/// `exp_inline` of pow.c: sign * exp(x + xtail).
#[inline(always)]
fn pow_exp(x: f64, xtail: f64, sign_bias: u32) -> f64 {
    let ix = x.to_bits();
    let mut abstop = ((ix >> 52) & 0x7ff) as u32;
    if abstop.wrapping_sub(0x3c9) >= 0x408 - 0x3c9 {
        if (abstop.wrapping_sub(0x3c9) as i32) < 0 {
            let one = x + 1.0;
            return if sign_bias != 0 { -one } else { one };
        }
        if abstop >= 0x409 {
            return if ix >> 63 != 0 { uflow(sign_bias) } else { oflow(sign_bias) };
        }
        abstop = 0;
    }
    let zs = fma(x, EXP_INVLN2N, EXP_SHIFT);
    let ki = zs.to_bits();
    let kd = zs - EXP_SHIFT;
    let r = xtail + fma(kd, EXP_NEGLN2LON, fma(kd, EXP_NEGLN2HIN, x));
    let idx = 2 * (ki & 127) as usize;
    let top = ki.wrapping_add(sign_bias as u64) << 45;
    let tail = asdouble(EXP_TAB[idx]);
    let sbits = top.wrapping_add(EXP_TAB[idx + 1]);
    let r2 = r * r;
    let a = r + tail;
    let b = fma(r2, fma(r, EXP_POLY[1], EXP_POLY[0]), a);
    let tmp = fma(r2 * r2, fma(r, EXP_POLY[3], EXP_POLY[2]), b);
    if abstop == 0 {
        return exp_specialcase(tmp, sbits, ki, true);
    }
    let scale = asdouble(sbits);
    fma(tmp, scale, scale)
}

/// glibc `pow` (FMA variant).
pub fn pow(x: f64, y: f64) -> f64 {
    let mut sign_bias: u32 = 0;
    let mut ix = x.to_bits();
    let iy = y.to_bits();
    let mut topx = (ix >> 52) as u32;
    let topy = (iy >> 52) as u32;
    if topx.wrapping_sub(0x001) >= 0x7ff - 0x001 || (topy & 0x7ff).wrapping_sub(0x3be) >= 0x43e - 0x3be {
        if zeroinfnan(iy) {
            if iy.wrapping_mul(2) == 0 {
                return if is_signaling(x) { x + y } else { 1.0 };
            }
            if ix == 1.0f64.to_bits() {
                return if is_signaling(y) { x + y } else { 1.0 };
            }
            if ix.wrapping_mul(2) > f64::INFINITY.to_bits().wrapping_mul(2) || iy.wrapping_mul(2) > f64::INFINITY.to_bits().wrapping_mul(2) {
                return x + y;
            }
            if ix.wrapping_mul(2) == 1.0f64.to_bits().wrapping_mul(2) {
                return 1.0;
            }
            if (ix.wrapping_mul(2) < 1.0f64.to_bits().wrapping_mul(2)) == (iy >> 63 == 0) {
                return 0.0;
            }
            return y * y;
        }
        if zeroinfnan(ix) {
            let mut x2 = x * x;
            if ix >> 63 != 0 && checkint(iy) == 1 {
                x2 = -x2;
            }
            return if iy >> 63 != 0 { 1.0 / x2 } else { x2 };
        }
        if ix >> 63 != 0 {
            let yint = checkint(iy);
            if yint == 0 {
                return invalid(x);
            }
            if yint == 1 {
                sign_bias = SIGN_BIAS;
            }
            ix &= 0x7fff_ffff_ffff_ffff;
            topx &= 0x7ff;
        }
        if (topy & 0x7ff).wrapping_sub(0x3be) >= 0x43e - 0x3be {
            if ix == 1.0f64.to_bits() {
                return 1.0;
            }
            if (topy & 0x7ff) < 0x3be {
                return if ix > 1.0f64.to_bits() { 1.0 + y } else { 1.0 - y };
            }
            return if (ix > 1.0f64.to_bits()) == (topy < 0x800) { oflow(0) } else { uflow(0) };
        }
        if topx == 0 {
            ix = (x * f64::from_bits(0x4330_0000_0000_0000)).to_bits();
            ix &= 0x7fff_ffff_ffff_ffff;
            ix = ix.wrapping_sub(52u64 << 52);
        }
    }
    let (hi, lo) = pow_log(ix);
    let ehi = y * hi;
    let elo = fma(y, lo, fma(y, hi, -ehi));
    pow_exp(ehi, elo, sign_bias)
}

/// `issignaling_inline` (IEEE 754-2008 quiet-bit convention, as on x86).
fn is_signaling(x: f64) -> bool {
    (x.to_bits() ^ 0x0008_0000_0000_0000).wrapping_mul(2) > 0x7ff8_0000_0000_0000u64.wrapping_mul(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn special_values() {
        assert_eq!(exp(0.0), 1.0);
        assert_eq!(exp(f64::NEG_INFINITY), 0.0);
        assert_eq!(exp(f64::INFINITY), f64::INFINITY);
        assert!(exp(f64::NAN).is_nan());
        assert_eq!(exp(1000.0), f64::INFINITY);
        assert_eq!(exp(-1000.0), 0.0);
        assert_eq!(log(1.0), 0.0);
        assert_eq!(log(0.0), f64::NEG_INFINITY);
        assert!(log(-1.0).is_nan());
        assert_eq!(log(f64::INFINITY), f64::INFINITY);
        assert_eq!(pow(2.0, 10.0), 1024.0);
        assert_eq!(pow(-2.0, 3.0), -8.0);
        assert!(pow(-2.0, 0.5).is_nan());
        assert_eq!(pow(0.0, -1.0), f64::INFINITY);
        assert_eq!(pow(-0.0, -1.0), f64::NEG_INFINITY);
        assert_eq!(pow(1.0, f64::NAN), 1.0);
        assert_eq!(pow(f64::NAN, 0.0), 1.0);
        assert_eq!(pow(10.0, 400.0), f64::INFINITY);
    }
}
