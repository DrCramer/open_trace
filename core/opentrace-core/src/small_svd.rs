//! Port of `genemapper_open/small_svd.py`: small least-squares solver (one-sided Jacobi, absolute monomials).
//! Operations follow the Python order exactly: `math.hypot`, `math.fsum`, `math.copysign`.
use crate::calibration::py_hypot;
use crate::fsum::fsum;
use serde_json::{json, Map, Value};

pub const SOLVER_ID: &str = "independent_binary64_one_sided_jacobi_absolute_monomials_v1";
pub const MAX_ROWS: usize = 4096;
pub const MAX_COLUMNS: usize = 3;
pub const MAX_ABS_MATRIX: f64 = 1e16;
pub const MAX_ABS_RHS: f64 = 1e16;
pub const MAX_ABS_X: f64 = 1e8;
pub const ORTHOGONALITY_TOLERANCE: f64 = 8.0 * f64::EPSILON;

pub type Res<T> = Result<T, String>;

/// Python `_finite_number(value, bound, label)`.
pub fn finite_number(v: &Value, bound: f64, label: &str) -> Res<f64> {
    if let Value::Number(n) = v {
        if let Some(x) = n.as_f64() {
            if x.is_finite() {
                if x.abs() > bound {
                    return Err(format!("{} exceeds the finite input bound", label));
                }
                return Ok(x);
            }
        }
    }
    Err(format!("{} must be a finite real number", label))
}

/// Sum that CPython raises on for finite inputs whose exact result overflows.
fn fsum_checked<I: IntoIterator<Item = f64>>(values: I) -> Res<f64> {
    let values: Vec<f64> = values.into_iter().collect();
    let all_finite = values.iter().all(|v| v.is_finite());
    let total = fsum(values.iter().copied());
    if all_finite && !total.is_finite() {
        return Err("overflow".into());
    }
    Ok(total)
}

fn norm(column: &[f64]) -> Res<f64> {
    let value = py_hypot(column);
    if !value.is_finite() {
        return Err("Column norm cannot be represented".into());
    }
    Ok(value)
}

fn correlation(left: &[f64], right: &[f64], left_norm: f64, right_norm: f64) -> Res<f64> {
    let value = fsum(left.iter().zip(right.iter()).map(|(&a, &b)| (a / left_norm) * (b / right_norm)));
    if !value.is_finite() {
        return Err("Column correlation cannot be represented".into());
    }
    Ok(value)
}

fn max_correlation(columns: &[Vec<f64>], norms: &[f64]) -> Res<f64> {
    let mut values = Vec::new();
    for p in 0..columns.len() {
        for q in p + 1..columns.len() {
            if norms[p] != 0.0 && norms[q] != 0.0 {
                values.push(correlation(&columns[p], &columns[q], norms[p], norms[q])?.abs());
            }
        }
    }
    Ok(values.iter().cloned().fold(0.0f64, |m, x| if x > m { x } else { m }))
}

pub struct Solve {
    pub coefficients: Vec<f64>,
    pub singular_values: Vec<f64>,
    pub nonzero: usize,
    pub sweeps: i64,
    pub max_column_correlation: f64,
    pub condition: Option<f64>,
    pub residual: f64,
}

/// Python `solve_least_squares` on already-validated numeric input (structure checked here).
pub fn solve_least_squares(matrix: &[Vec<f64>], rhs: &[f64], max_sweeps: i64) -> Res<Solve> {
    if !(1..=256).contains(&max_sweeps) {
        return Err("max_sweeps must be an integer in1..256".into());
    }
    if matrix.is_empty() || matrix.len() > MAX_ROWS {
        return Err("A bounded nonempty row matrix is required".into());
    }
    let size = matrix[0].len();
    if !(1..=MAX_COLUMNS).contains(&size) {
        return Err("Matrix must have1..3 columns".into());
    }
    if matrix.len() < size {
        return Err("This independent solver requires rows>=columns".into());
    }
    if rhs.len() != matrix.len() {
        return Err("One explicit right-hand value per row is required".into());
    }
    if matrix.iter().any(|r| r.len() != size) {
        return Err("Matrix rows must have equal length".into());
    }
    let mut columns: Vec<Vec<f64>> = (0..size).map(|j| matrix.iter().map(|r| r[j]).collect()).collect();
    // vectors[c][r]: identity start, rotated alongside the columns.
    let mut vectors: Vec<Vec<f64>> = (0..size)
        .map(|c| (0..size).map(|r| if r == c { 1.0 } else { 0.0 }).collect())
        .collect();
    let mut sweeps: i64 = 0;
    let mut norms: Vec<f64> = columns.iter().map(|c| norm(c)).collect::<Res<Vec<f64>>>()?;
    let mut largest = max_correlation(&columns, &norms)?;
    while largest > ORTHOGONALITY_TOLERANCE && sweeps < max_sweeps {
        for p in 0..size {
            for q in p + 1..size {
                let left = columns[p].clone();
                let right = columns[q].clone();
                let np = norm(&left)?;
                let nq = norm(&right)?;
                if np == 0.0 || nq == 0.0 {
                    continue;
                }
                let corr = correlation(&left, &right, np, nq)?;
                if corr.abs() <= ORTHOGONALITY_TOLERANCE {
                    continue;
                }
                let zeta = (nq / np - np / nq) / (2.0 * corr);
                let tangent = if zeta.is_finite() {
                    if zeta == 0.0 {
                        1.0
                    } else {
                        let root_denominator = zeta.abs() + py_hypot(&[1.0, zeta]);
                        if root_denominator.is_finite() {
                            1.0f64.copysign(zeta) / root_denominator
                        } else {
                            let reciprocal = 1.0 / zeta.abs();
                            (reciprocal / (1.0 + py_hypot(&[1.0, reciprocal]))).copysign(zeta)
                        }
                    }
                } else {
                    let norm_scale = if nq > np { nq } else { np };
                    let rp = np / norm_scale;
                    let rq = nq / norm_scale;
                    let difference = rq * rq - rp * rp;
                    let twice_cross = 2.0 * corr * rp * rq;
                    let denominator = difference + py_hypot(&[difference, twice_cross]).copysign(difference);
                    twice_cross / denominator
                };
                if !tangent.is_finite() || tangent == 0.0 {
                    return Err("Jacobi angle cannot be represented".into());
                }
                let cosine = 1.0 / py_hypot(&[1.0, tangent]);
                let sine = cosine * tangent;
                columns[p] = left
                    .iter()
                    .zip(right.iter())
                    .map(|(&a, &b)| fsum([cosine * a, (-sine) * b]))
                    .collect();
                columns[q] = left
                    .iter()
                    .zip(right.iter())
                    .map(|(&a, &b)| fsum([sine * a, cosine * b]))
                    .collect();
                let vp = vectors[p].clone();
                let vq = vectors[q].clone();
                vectors[p] = vp.iter().zip(vq.iter()).map(|(&a, &b)| fsum([cosine * a, (-sine) * b])).collect();
                vectors[q] = vp.iter().zip(vq.iter()).map(|(&a, &b)| fsum([sine * a, cosine * b])).collect();
            }
        }
        sweeps += 1;
        norms = columns.iter().map(|c| norm(c)).collect::<Res<Vec<f64>>>()?;
        largest = max_correlation(&columns, &norms)?;
    }
    if largest > ORTHOGONALITY_TOLERANCE {
        return Err("One-sided Jacobi SVD did not converge within max_sweeps".into());
    }
    let back = || -> Res<(Vec<f64>, Vec<f64>, f64)> {
        let mut projections = Vec::with_capacity(size);
        for (column, &singular) in columns.iter().zip(norms.iter()) {
            if singular == 0.0 {
                projections.push(0.0);
            } else {
                let s = fsum_checked(column.iter().zip(rhs.iter()).map(|(&value, &target)| (value / singular) * target))?;
                projections.push(s / singular);
            }
        }
        let mut coefficients = Vec::with_capacity(size);
        for row in 0..size {
            coefficients.push(fsum_checked(vectors.iter().zip(projections.iter()).map(|(v, &p)| v[row] * p))?);
        }
        let mut residuals = Vec::with_capacity(matrix.len());
        for (row, &target) in matrix.iter().zip(rhs.iter()) {
            let dot = fsum_checked(row.iter().zip(coefficients.iter()).map(|(&a, &b)| a * b))?;
            residuals.push(dot - target);
        }
        let residual = py_hypot(&residuals);
        Ok((coefficients, projections, residual))
    };
    let (coefficients, projections, residual) =
        back().map_err(|_| "SVD back-substitution or residual cannot be represented".to_string())?;
    let finite = coefficients.iter().chain(projections.iter()).chain([residual].iter()).all(|v| v.is_finite());
    if !finite {
        return Err("SVD back-substitution or residual is nonfinite".into());
    }
    let nonzero: Vec<f64> = norms.iter().cloned().filter(|&v| v != 0.0).collect();
    let condition = if !nonzero.is_empty() && nonzero.len() == size {
        let hi = nonzero.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let lo = nonzero.iter().cloned().fold(f64::INFINITY, f64::min);
        let c = hi / lo;
        if c.is_finite() { Some(c) } else { None }
    } else {
        None
    };
    Ok(Solve {
        coefficients,
        singular_values: norms,
        nonzero: nonzero.len(),
        sweeps,
        max_column_correlation: largest,
        condition,
        residual,
    })
}

fn fv(x: f64) -> Value {
    serde_json::Number::from_f64(x).map(Value::Number).unwrap_or(Value::Null)
}

/// Diagnostics dictionary of Python `solve_least_squares`.
pub fn solve_to_value(s: &Solve, size: usize) -> Value {
    let mut m = Map::new();
    m.insert("coefficients".into(), Value::Array(s.coefficients.iter().map(|&v| fv(v)).collect()));
    m.insert("solver".into(), json!(SOLVER_ID));
    m.insert("singular_values".into(), Value::Array(s.singular_values.iter().map(|&v| fv(v)).collect()));
    m.insert("computed_nonzero_singular_values".into(), json!(s.nonzero));
    m.insert("exact_zero_singular_values".into(), json!(size - s.nonzero));
    m.insert("sweeps".into(), json!(s.sweeps));
    m.insert("max_column_correlation".into(), fv(s.max_column_correlation));
    m.insert("orthogonality_tolerance".into(), fv(ORTHOGONALITY_TOLERANCE));
    m.insert("singular_value_policy".into(), json!("skip_exact_zero_only_no_rcond"));
    m.insert("condition_estimate".into(), s.condition.map(fv).unwrap_or(Value::Null));
    m.insert("residual_l2".into(), fv(s.residual));
    m.insert("original_runtime_validated".into(), json!(false));
    m.insert("external_validation".into(), json!(false));
    Value::Object(m)
}

/// Python `polynomial_fit(x, y, degree)` on validated coordinates: absolute monomials, ascending coefficients.
pub fn polynomial_fit(x: &[f64], y: &[f64], degree: usize, max_sweeps: i64) -> Res<Value> {
    if degree != 1 && degree != 2 {
        return Err("Only explicit polynomial degree1 or2 is supported".into());
    }
    if x.len() != y.len() || !(degree + 1 <= x.len() && x.len() <= MAX_ROWS) {
        return Err("A bounded list/tuple of paired polynomial observations is required".into());
    }
    let matrix: Vec<Vec<f64>> = x
        .iter()
        .map(|&v| if degree == 1 { vec![1.0, v] } else { vec![1.0, v, v * v] })
        .collect();
    let s = solve_least_squares(&matrix, y, max_sweeps)?;
    let mut m = match solve_to_value(&s, degree + 1) {
        Value::Object(m) => m,
        _ => unreachable!(),
    };
    m.insert("polynomial_degree".into(), json!(degree));
    m.insert("basis".into(), json!("absolute_scan_monomials_ascending"));
    m.insert("centered".into(), json!(false));
    m.insert("column_preconditioned".into(), json!(false));
    m.insert("regularized".into(), json!(false));
    Ok(Value::Object(m))
}

/// Python `solve_least_squares` with raw JSON-like inputs: element types and bounds are checked as in Python.
pub fn solve_value(matrix: &Value, rhs: &Value, max_sweeps: &Value) -> Res<Value> {
    let sweeps = match max_sweeps {
        Value::Number(n) if n.is_u64() || n.is_i64() => n.as_i64().unwrap_or(0),
        _ => return Err("max_sweeps must be an integer in1..256".into()),
    };
    let rows_v = match matrix {
        Value::Array(a) if !a.is_empty() && a.len() <= MAX_ROWS => a,
        _ => return Err("A bounded nonempty row matrix is required".into()),
    };
    let size = match &rows_v[0] {
        Value::Array(r) if (1..=MAX_COLUMNS).contains(&r.len()) => r.len(),
        _ => return Err("Matrix must have1..3 columns".into()),
    };
    if rows_v.len() < size {
        return Err("This independent solver requires rows>=columns".into());
    }
    let rhs_v = match rhs {
        Value::Array(a) if a.len() == rows_v.len() => a,
        _ => return Err("One explicit right-hand value per row is required".into()),
    };
    let mut rows: Vec<Vec<f64>> = Vec::new();
    for row in rows_v {
        let r = match row {
            Value::Array(r) if r.len() == size => r,
            _ => return Err("Matrix rows must have equal length".into()),
        };
        rows.push(r.iter().map(|v| finite_number(v, MAX_ABS_MATRIX, "Matrix entry")).collect::<Res<Vec<f64>>>()?);
    }
    let values: Vec<f64> = rhs_v.iter().map(|v| finite_number(v, MAX_ABS_RHS, "Right-hand value")).collect::<Res<_>>()?;
    let s = solve_least_squares(&rows, &values, sweeps)?;
    Ok(solve_to_value(&s, size))
}

/// Python `polynomial_fit` with raw inputs (bounds on x and y checked here).
pub fn polynomial_value(x: &Value, y: &Value, degree: &Value, max_sweeps: &Value) -> Res<Value> {
    let degree = match degree {
        Value::Number(n) if n.is_u64() => n.as_u64().unwrap() as usize,
        _ => return Err("Only explicit polynomial degree1 or2 is supported".into()),
    };
    let (xs, ys) = match (x, y) {
        (Value::Array(a), Value::Array(b)) if a.len() == b.len() => (a, b),
        _ => return Err("A bounded list/tuple of paired polynomial observations is required".into()),
    };
    let coords: Vec<f64> = xs.iter().map(|v| finite_number(v, MAX_ABS_X, "Polynomial x")).collect::<Res<_>>()?;
    let values: Vec<f64> = ys.iter().map(|v| finite_number(v, MAX_ABS_RHS, "Right-hand value")).collect::<Res<_>>()?;
    let sweeps = match max_sweeps {
        Value::Number(n) if n.is_i64() || n.is_u64() => n.as_i64().unwrap_or(0),
        _ => 64,
    };
    polynomial_fit(&coords, &values, degree, sweeps)
}
