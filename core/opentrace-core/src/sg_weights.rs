//! Port of `sg_peaks.sg_derivative_weights(window, degree)`: general Savitzky-Golay first-derivative weights.
//!
//! The Python reference solves the odd-power normal equations by Gauss-Jordan elimination over exact rationals
//! and converts each final weight with `float(Fraction)`, which is correctly rounded. This port follows the same
//! elimination order with `BigRational` and rounds each exact quotient to nearest, ties to even, so the results
//! are bit-identical to the reference.
use num_bigint::{BigInt, BigUint, Sign};
use num_rational::BigRational;
use num_traits::{One, Signed, ToPrimitive, Zero};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// Multiply by 2^e exactly (normal range).
fn ldexp(mut x: f64, mut e: i64) -> f64 {
    while e > 1000 {
        x *= 2f64.powi(1000);
        e -= 1000;
    }
    while e < -1000 {
        x *= 2f64.powi(-1000);
        e += 1000;
    }
    x * 2f64.powi(e as i32)
}

/// Correctly rounded (nearest, ties to even) value of num/den. `den` must be nonzero.
pub fn ratio_to_f64(num: &BigInt, den: &BigInt) -> f64 {
    if num.is_zero() {
        return 0.0;
    }
    let neg = (num.sign() == Sign::Minus) != (den.sign() == Sign::Minus);
    let a: BigUint = num.abs().to_biguint().unwrap();
    let b: BigUint = den.abs().to_biguint().unwrap();
    let e: i64 = a.bits() as i64 - b.bits() as i64;
    // Scale so the integer quotient has 55 or 56 bits: two guard bits beyond 53 plus a sticky remainder.
    let s: i64 = 55 - e;
    let (num_s, den_s): (BigUint, BigUint) = if s >= 0 {
        (a << (s as usize), b)
    } else {
        let shift = (0 - s) as usize;
        (a, b << shift)
    };
    let q = &num_s / &den_s;
    let r = &num_s % &den_s;
    let l = q.bits() as i64;
    let extra = l - 53;
    let mant = &q >> (extra as usize);
    let rem = &q - (&mant << (extra as usize));
    let half = BigUint::one() << ((extra - 1) as usize);
    let up = rem > half || (rem == half && (!r.is_zero() || mant.bit(0)));
    let m = if up { mant + 1u32 } else { mant };
    let value = ldexp(m.to_u64().unwrap() as f64, extra - s);
    if neg { -value } else { value }
}

fn rational_to_f64(x: &BigRational) -> f64 {
    ratio_to_f64(x.numer(), x.denom())
}

type WeightCache = Mutex<HashMap<(usize, usize), Result<Vec<f64>, String>>>;

fn cache() -> &'static WeightCache {
    static CACHE: OnceLock<WeightCache> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Python `sg_derivative_weights(window, degree)`, memoized per process: the weights depend only on
/// (window, degree), and the server reuses the same detector settings for every sample.
pub fn sg_derivative_weights(window: usize, degree: usize) -> Result<Vec<f64>, String> {
    let cached = cache().lock().unwrap().get(&(window, degree)).cloned();
    if let Some(result) = cached {
        return result;
    }
    let result = solve_weights(window, degree);
    cache().lock().unwrap().insert((window, degree), result.clone());
    result
}

/// Weights w_k for k = 1..radius (radius = window // 2), solved exactly.
fn solve_weights(window: usize, degree: usize) -> Result<Vec<f64>, String> {
    let radius = window / 2;
    let powers: Vec<usize> = (1..=degree).step_by(2).collect();
    if powers.is_empty() || powers.len() > radius {
        return Err("SG degree must be >= 1 and leave at least one degree of freedom in the window".into());
    }
    let m = powers.len();
    // Normal matrix on the odd-power subspace: A[a][b] = 2 * sum_k k^(p_a + p_b).
    // Power sums S_e = sum_k k^e, each exponent e = p_a + p_b computed once by repeated multiplication.
    let max_e = 2 * powers[m - 1];
    let mut power_sums: Vec<BigInt> = vec![BigInt::zero(); max_e + 1];
    let mut kpow: Vec<BigInt> = vec![BigInt::one(); radius + 1];
    for sum in power_sums.iter_mut() {
        for k in 1..=radius {
            *sum += &kpow[k];
            kpow[k] *= k;
        }
    }
    let mut matrix: Vec<Vec<BigRational>> = Vec::with_capacity(m);
    for &pa in &powers {
        let row: Vec<BigRational> = powers
            .iter()
            .map(|&pb| BigRational::from_integer(&power_sums[pa + pb] * 2))
            .collect();
        matrix.push(row);
    }
    // Gauss-Jordan on [A | I]; the first row of the inverse holds the linear-term coefficients.
    let mut rhs: Vec<Vec<BigRational>> = (0..m)
        .map(|i| (0..m).map(|j| BigRational::from_integer(BigInt::from(i32::from(i == j)))).collect())
        .collect();
    for col in 0..m {
        let pivot = (col..m)
            .find(|&r| !matrix[r][col].is_zero())
            .ok_or_else(|| "singular SG normal matrix".to_string())?;
        matrix.swap(col, pivot);
        rhs.swap(col, pivot);
        let inv = matrix[col][col].recip();
        for v in matrix[col].iter_mut() {
            *v = &*v * &inv;
        }
        for v in rhs[col].iter_mut() {
            *v = &*v * &inv;
        }
        for r in 0..m {
            if r != col && !matrix[r][col].is_zero() {
                let f = matrix[r][col].clone();
                let pivot_row = matrix[col].clone();
                for (a, b) in matrix[r].iter_mut().zip(pivot_row.iter()) {
                    *a = &*a - &f * b;
                }
                let pivot_rhs = rhs[col].clone();
                for (a, b) in rhs[r].iter_mut().zip(pivot_rhs.iter()) {
                    *a = &*a - &f * b;
                }
            }
        }
    }
    let row = &rhs[0];
    let mut weights = Vec::with_capacity(radius);
    for k in 1..=radius {
        let mut total = BigRational::zero();
        for (j, &p) in powers.iter().enumerate() {
            total += &row[j] * BigRational::from_integer(BigInt::from(k).pow(p as u32));
        }
        weights.push(rational_to_f64(&total));
    }
    Ok(weights)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ratio_rounding_matches_python_division() {
        assert_eq!(ratio_to_f64(&BigInt::from(1), &BigInt::from(3)), 1.0 / 3.0);
        assert_eq!(ratio_to_f64(&BigInt::from(-2), &BigInt::from(3)), -2.0 / 3.0);
        assert_eq!(ratio_to_f64(&BigInt::from(7), &BigInt::from(2)), 3.5);
        assert!(Signed::is_negative(&BigInt::from(-1)));
    }

    #[test]
    fn degree_one_is_the_linear_slope() {
        // Window 5 (radius 2): slope weights k / (2 * sum k^2) = k / 10.
        let w = sg_derivative_weights(5, 1).unwrap();
        assert_eq!(w.len(), 2);
        assert!((w[0] - 0.1).abs() < 1e-15);
        assert!((w[1] - 0.2).abs() < 1e-15);
    }
}
