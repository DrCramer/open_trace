//! Exact Python `statistics.mean` / `statistics.stdev` (3.12+/3.13 algorithms) for lists of floats.
use num_bigint::{BigInt, Sign};
use num_rational::BigRational;
use num_traits::{ToPrimitive, Zero};

fn ldexp(mut m: f64, mut k: i64) -> f64 {
    while k > 1000 {
        m *= 2f64.powi(1000);
        k -= 1000;
    }
    while k < -1000 {
        m *= 2f64.powi(-1000);
        k += 1000;
    }
    m * 2f64.powi(k as i32)
}

/// Python `int / int` (correctly rounded true division); denominator must be positive.
pub fn ratio_to_f64(num: &BigInt, den: &BigInt) -> Result<f64, String> {
    if num.is_zero() {
        return Ok(0.0);
    }
    let neg = num.sign() == Sign::Minus;
    let a = num.magnitude().clone();
    let b = den.magnitude().clone();
    let e = a.bits() as i64 - b.bits() as i64;
    let shift = 55 - e;
    let (a, b) = if shift >= 0 { (a << (shift as usize), b) } else { (a, b << ((-shift) as usize)) };
    let q = &a / &b;
    let rem = &a - &q * &b;
    let sticky = !rem.is_zero();
    let l = q.bits() as i64;
    let lead = l - 1 - shift;
    let mbits = if lead < -1022 { 53 - (-1022 - lead) } else { 53 };
    let drop = l - mbits;
    let (mant, rounded_exp) = if drop <= 0 {
        (q, -shift)
    } else {
        let mant = &q >> (drop as usize);
        let low = &q - (&mant << (drop as usize));
        let half = num_bigint::BigUint::from(1u8) << ((drop - 1) as usize);
        let up = if low > half {
            true
        } else if low == half {
            sticky || mant.bit(0)
        } else {
            false
        };
        (if up { mant + 1u8 } else { mant }, drop - shift)
    };
    let m = mant.to_f64().unwrap();
    let v = ldexp(m, rounded_exp);
    if v.is_infinite() {
        return Err("integer division result too large for a float".into());
    }
    Ok(if neg { -v } else { v })
}

fn exact(x: f64) -> Result<BigRational, String> {
    BigRational::from_float(x).ok_or_else(|| "non-finite value".to_string())
}

pub fn mean(values: &[f64]) -> Result<f64, String> {
    if values.is_empty() {
        return Err("mean requires at least one data point".into());
    }
    let mut total = BigRational::zero();
    for &v in values {
        total += exact(v)?;
    }
    let r = total / BigRational::from_integer(BigInt::from(values.len()));
    ratio_to_f64(r.numer(), r.denom())
}

fn isqrt_rto(n: &BigInt, m: &BigInt) -> BigInt {
    let a = (n / m).sqrt();
    if &(&a * &a * m) != n {
        a | BigInt::from(1)
    } else {
        a
    }
}

fn float_sqrt_of_frac(n: &BigInt, m: &BigInt) -> Result<f64, String> {
    let q = (n.bits() as i64 - m.bits() as i64 - 109).div_euclid(2);
    if q >= 0 {
        let numerator = isqrt_rto(n, &(m << ((2 * q) as usize))) << (q as usize);
        ratio_to_f64(&numerator, &BigInt::from(1))
    } else {
        let numerator = isqrt_rto(&(n << ((-2 * q) as usize)), m);
        ratio_to_f64(&numerator, &(BigInt::from(1) << ((-q) as usize)))
    }
}

pub fn stdev(values: &[f64]) -> Result<f64, String> {
    if values.len() < 2 {
        return Err("stdev requires at least two data points".into());
    }
    let mut sx = BigRational::zero();
    let mut sxx = BigRational::zero();
    for &v in values {
        let x = exact(v)?;
        sxx += &x * &x;
        sx += x;
    }
    let count = BigRational::from_integer(BigInt::from(values.len()));
    let ssd = (&count * &sxx - &sx * &sx) / &count;
    let mss = ssd / BigRational::from_integer(BigInt::from(values.len() - 1));
    float_sqrt_of_frac(mss.numer(), mss.denom())
}
