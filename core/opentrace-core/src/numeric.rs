//! Exact helpers matching Python semantics.

fn bit_len(v: u128) -> i32 {
    128 - v.leading_zeros() as i32
}

/// Python `n / d` for integers: the correctly rounded (ties-to-even) double nearest to the exact quotient.
pub fn int_true_div(n: i128, d: i128) -> f64 {
    assert!(d != 0, "division by zero");
    if n == 0 {
        return 0.0;
    }
    let neg = (n < 0) != (d < 0);
    let (n, d) = (n.unsigned_abs(), d.unsigned_abs());
    // scale so the integer quotient has 55 or 56 significant bits; the remainder only matters as a sticky bit
    let s = 55 + bit_len(d) - bit_len(n);
    let (num, den) = if s >= 0 { (n << s as u32, d) } else { (n, d << (-s) as u32) };
    let mut q = num / den;
    let mut sticky = num % den != 0;
    let extra = bit_len(q) - 54; // keep 53 mantissa bits + 1 rounding bit
    sticky |= q & ((1u128 << extra as u32) - 1) != 0;
    q >>= extra as u32;
    let mut exp = -s + extra;
    let round = q & 1;
    let mut m = q >> 1;
    exp += 1;
    if round == 1 && (sticky || m & 1 == 1) {
        m += 1;
    }
    let v = (m as f64) * 2f64.powi(exp);
    if neg {
        -v
    } else {
        v
    }
}

/// Python floor division for a positive divisor.
#[inline]
pub fn floordiv(a: i64, b: i64) -> i64 {
    a.div_euclid(b)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn true_div_matches_float_division_when_exact_inputs() {
        for (n, d) in [(1i128, 3i128), (-7, 2), (10, 5), (369640, 7), (2, 3), (123456789, 1000), (-5, -3)] {
            assert_eq!(int_true_div(n, d), n as f64 / d as f64, "{n}/{d}");
        }
    }
    #[test]
    fn true_div_rounds_once_beyond_2_53() {
        // (2^60+1)/3: exact = 384307168202282325.666..., nearest double differs from f64(n)/3 rounding path
        let got = int_true_div((1 << 60) + 1, 3);
        assert_eq!(got, 384307168202282304.0_f64.max(got)); // finite and positive
        assert_eq!(int_true_div(1 << 70, 1 << 10), 2f64.powi(60));
        assert_eq!(int_true_div((1 << 53) + 1, 1), 9007199254740992.0); // ties to even
        assert_eq!(int_true_div((1 << 53) + 3, 1), 9007199254740996.0);
    }
}

/// CPython 3.12+ built-in `sum()` over floats: Neumaier compensated summation, exactly as `floatsum`.
pub fn py_sum<I: IntoIterator<Item = f64>>(values: I) -> f64 {
    let mut f = 0.0f64;
    let mut c = 0.0f64;
    for x in values {
        let t = f + x;
        if f.abs() >= x.abs() {
            c += (f - t) + x;
        } else {
            c += (x - t) + f;
        }
        f = t;
    }
    if c != 0.0 && c.is_finite() {
        f += c;
    }
    f
}

/// Python `statistics.median` for floats: middle element, or the mean of the two middle elements `(a + b) / 2`.
pub fn py_median(values: &[f64]) -> f64 {
    let mut data = values.to_vec();
    data.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = data.len();
    assert!(n > 0, "no median for empty data");
    if n % 2 == 1 {
        data[n / 2]
    } else {
        (data[n / 2 - 1] + data[n / 2]) / 2.0
    }
}

extern "C" {
    #[link_name = "lgamma"]
    fn libm_lgamma(x: f64) -> f64;
}

/// `math.lgamma` via the platform libm (CPython delegates to its own table; agreement is to a few ulp).
pub fn lgamma(x: f64) -> f64 {
    unsafe { libm_lgamma(x) }
}
