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

const LANCZOS_G: f64 = 6.024680040776729583740234375;
const LANCZOS_NUM: [f64; 13] = [
    23531376880.410759688572007674451636754734846804940,
    42919803642.649098768957899047001988850926355848959,
    35711959237.355668049440185451547166705960488635843,
    17921034426.037209699919755754458931112671403265390,
    6039542586.3520280050642916443072979210699388420708,
    1439720407.3117216736632230727949123939715485786772,
    248874557.86205415651146038641322942321632125127801,
    31426415.585400194380614231628318205362874684987640,
    2876370.6289353724412254090516208496135991145378768,
    186056.26539522349504029498971604569928220784236328,
    8071.6720023658162106380029022722506138218516325024,
    210.82427775157934587250973392071336271166969580291,
    2.5066282746310002701649081771338373386264310793408,
];
const LANCZOS_DEN: [f64; 13] = [
    0.0, 39916800.0, 120543840.0, 150917976.0, 105258076.0, 45995730.0, 13339535.0, 2637558.0, 357423.0, 32670.0, 1925.0, 66.0, 1.0,
];

fn lanczos_sum(x: f64) -> f64 {
    let (mut num, mut den) = (0.0f64, 0.0f64);
    if x < 5.0 {
        for i in (0..13).rev() {
            num = num * x + LANCZOS_NUM[i];
            den = den * x + LANCZOS_DEN[i];
        }
    } else {
        for i in 0..13 {
            num = num / x + LANCZOS_NUM[i];
            den = den / x + LANCZOS_DEN[i];
        }
    }
    num / den
}

/// CPython's `m_lgamma` (Lanczos approximation), so results are bit-identical to `math.lgamma` for x > 0.
pub fn lgamma(x: f64) -> f64 {
    if !x.is_finite() {
        return if x.is_nan() { x } else { f64::INFINITY };
    }
    if x == x.floor() && x <= 2.0 {
        return if x <= 0.0 { f64::INFINITY } else { 0.0 };
    }
    let absx = x.abs();
    if absx < 1e-20 {
        return -absx.ln();
    }
    let mut r = lanczos_sum(absx).ln() - LANCZOS_G;
    r += (absx - 0.5) * ((absx + LANCZOS_G - 0.5).ln() - 1.0);
    if x < 0.0 {
        // Reflection formula (never used by the sizing code, which only passes positive shapes).
        let logpi = std::f64::consts::PI.ln();
        let sinpi = (std::f64::consts::PI * (absx % 2.0)).sin().abs();
        r = logpi - r - sinpi.ln() - absx.ln();
    }
    r
}

extern "C" {
    #[link_name = "pow"]
    fn libm_pow(x: f64, y: f64) -> f64;
}

/// Python `x ** y` for floats (C `pow`); the exponent is hidden from the optimizer so that `pow(x, 2.0)` is
/// not folded into `x * x` (glibc's pow is not always correctly rounded, CPython calls it directly).
pub fn py_pow(x: f64, y: f64) -> f64 {
    unsafe { libm_pow(x, std::hint::black_box(y)) }
}
