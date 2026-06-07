use std::sync::Arc;

use num_complex::Complex64;
use rayon::prelude::*;

use super::{
    cas_err, cas_sub, eval_exact_numeric_div, eval_numeric_binary, numeric_is_zero, poly_degree,
    poly_from_expr, simplify_cas_value, split_add_term, var_name_from_value,
};
use crate::value::cas::CasOp;
use crate::value::{Value, WqResult};

fn complex_to_value(z: Complex64) -> Value {
    if z.im.abs() <= 1e-12 {
        Value::float(z.re)
    } else {
        Value::from_complex64(z)
    }
}

fn solve_monomial_polynomial(coeffs: &[Value], degree: usize) -> WqResult<Vec<Value>> {
    if degree >= coeffs.len() {
        return Err(cas_err(format!(
            "degree {degree} exceeds polynomial coefficient count {}",
            coeffs.len()
        )));
    }
    if coeffs[1..degree]
        .iter()
        .any(|coeff| !numeric_is_zero(coeff))
    {
        return Err(cas_err(format!(
            "solve currently supports degree {degree} only for equations of the form a*x^{degree} + b = 0"
        )));
    }
    let leading = coeffs[degree]
        .try_as_complex64()
        .map_err(|e| e.src("cas"))?;
    if leading == Complex64::new(0.0, 0.0) {
        return Err(cas_err("leading coefficient cannot be zero"));
    }
    let constant = coeffs[0].try_as_complex64().map_err(|e| e.src("cas"))?;
    let target = -constant / leading;
    let radius = target.norm().powf(1.0 / degree as f64);
    let angle = target.arg();
    let roots: Vec<Value> = (0..degree)
        .into_par_iter()
        .map(|k| {
            let theta = (angle + 2.0 * std::f64::consts::PI * k as f64) / degree as f64;
            complex_to_value(Complex64::from_polar(radius, theta))
        })
        .collect();
    Ok(roots)
}

/// Cubic formula (Cardano). Returns 3 roots of `a*x³ + b*x² + c*x + d = 0`.
/// Coefficients must be numeric (integers, floats, or fractions).
#[allow(dead_code)]
pub(crate) fn solve_cubic(coeffs: &[Value]) -> WqResult<Vec<Value>> {
    let a = coeffs.get(3).cloned().unwrap_or(Value::Int(0));
    let b = coeffs.get(2).cloned().unwrap_or(Value::Int(0));
    let c = coeffs.get(1).cloned().unwrap_or(Value::Int(0));
    let d = coeffs.first().cloned().unwrap_or(Value::Int(0));

    if numeric_is_zero(&a) {
        return Err(cas_err("solve_cubic: leading coefficient is zero"));
    }

    // Depress: substitute x = t - b/(3a) → t³ + p·t + q = 0
    let a_f = a
        .as_f64()
        .ok_or_else(|| cas_err("solve_cubic: non-numeric coefficient"))?;
    let b_f = b
        .as_f64()
        .ok_or_else(|| cas_err("solve_cubic: non-numeric coefficient"))?;
    let c_f = c
        .as_f64()
        .ok_or_else(|| cas_err("solve_cubic: non-numeric coefficient"))?;
    let d_f = d
        .as_f64()
        .ok_or_else(|| cas_err("solve_cubic: non-numeric coefficient"))?;

    let b_over_3a = b_f / (3.0 * a_f);

    // p = (3ac - b²) / (3a²)
    let p = (3.0 * a_f * c_f - b_f * b_f) / (3.0 * a_f * a_f);
    // q = (2b³ - 9abc + 27a²d) / (27a³)
    let q =
        (2.0 * b_f.powi(3) - 9.0 * a_f * b_f * c_f + 27.0 * a_f * a_f * d_f) / (27.0 * a_f.powi(3));

    // Discriminant: Δ = (q/2)² + (p/3)³
    let disc = (q / 2.0).powi(2) + (p / 3.0).powi(3);

    let roots: Vec<Complex64> = if disc.abs() < 1e-12 {
        // Multiple root
        let u = -q / 2.0;
        let u_cbrt = if u >= 0.0 {
            u.powf(1.0 / 3.0)
        } else {
            -(-u).powf(1.0 / 3.0)
        };
        let t0 = 2.0 * u_cbrt;
        let t1 = -u_cbrt;
        vec![
            Complex64::new(t0, 0.0),
            Complex64::new(t1, 0.0),
            Complex64::new(t1, 0.0),
        ]
    } else if disc > 0.0 {
        // One real root
        let sqrt_disc = disc.sqrt();
        let u = -q / 2.0 + sqrt_disc;
        let v = -q / 2.0 - sqrt_disc;
        let u_cbrt = if u >= 0.0 {
            u.powf(1.0 / 3.0)
        } else {
            -(-u).powf(1.0 / 3.0)
        };
        let v_cbrt = if v >= 0.0 {
            v.powf(1.0 / 3.0)
        } else {
            -(-v).powf(1.0 / 3.0)
        };
        let t0 = u_cbrt + v_cbrt;
        let t_real = -0.5 * (u_cbrt + v_cbrt);
        let t_imag = 0.5 * f64::sqrt(3.0) * (u_cbrt - v_cbrt);
        vec![
            Complex64::new(t0, 0.0),
            Complex64::new(t_real, t_imag),
            Complex64::new(t_real, -t_imag),
        ]
    } else {
        // Casus irreducibilis: three real roots via trig formula
        // tₖ = 2√(-p/3)·cos(arccos(3q√(-3/p)/(2p)) / 3 + 2πk/3)
        let r = 2.0 * (-p / 3.0).sqrt();
        let arg = (3.0 * q) / (2.0 * p) * (-3.0 / p).sqrt();
        // Clamp to [-1, 1] for numerical stability
        let theta = arg.clamp(-1.0, 1.0).acos() / 3.0;
        (0..3)
            .map(|k| {
                let t = r * (theta + 2.0 * std::f64::consts::PI * k as f64 / 3.0).cos();
                Complex64::new(t, 0.0)
            })
            .collect()
    };

    // Undo depression: x = t - b/(3a)
    Ok(roots
        .into_iter()
        .map(|t| {
            let x = t - Complex64::new(b_over_3a, 0.0);
            complex_to_value(x)
        })
        .collect())
}

/// Quartic formula (Ferrari). Returns 4 roots of `a*x⁴ + b*x³ + c*x² + d*x + e
/// = 0`. Coefficients must be numeric.
#[allow(dead_code)]
pub(crate) fn solve_quartic(coeffs: &[Value]) -> WqResult<Vec<Value>> {
    let a = coeffs.get(4).cloned().unwrap_or(Value::Int(0));
    let b = coeffs.get(3).cloned().unwrap_or(Value::Int(0));
    let c = coeffs.get(2).cloned().unwrap_or(Value::Int(0));
    let d = coeffs.get(1).cloned().unwrap_or(Value::Int(0));
    let e = coeffs.first().cloned().unwrap_or(Value::Int(0));

    if numeric_is_zero(&a) {
        return Err(cas_err("solve_quartic: leading coefficient is zero"));
    }

    let a_f = a
        .as_f64()
        .ok_or_else(|| cas_err("solve_quartic: non-numeric coefficient"))?;
    let b_f = b
        .as_f64()
        .ok_or_else(|| cas_err("solve_quartic: non-numeric coefficient"))?;
    let c_f = c
        .as_f64()
        .ok_or_else(|| cas_err("solve_quartic: non-numeric coefficient"))?;
    let d_f = d
        .as_f64()
        .ok_or_else(|| cas_err("solve_quartic: non-numeric coefficient"))?;
    let e_f = e
        .as_f64()
        .ok_or_else(|| cas_err("solve_quartic: non-numeric coefficient"))?;

    // Normalise and depress: x = t - b/(4a), divide by a
    let b_n = b_f / a_f;
    let c_n = c_f / a_f;
    let d_n = d_f / a_f;
    let e_n = e_f / a_f;

    let b_over_4 = b_n / 4.0;

    // Depressed quartic: t⁴ + p·t² + q·t + r = 0
    let p = c_n - 3.0 * b_n.powi(2) / 8.0;
    let q = d_n - b_n * c_n / 2.0 + b_n.powi(3) / 8.0;
    let r = e_n - b_n * d_n / 4.0 + b_n.powi(2) * c_n / 16.0 - 3.0 * b_n.powi(4) / 256.0;

    // Resolvent cubic: 8m³ - 4p·m² - 8r·m + (4pr - q²) = 0
    //                  m³ - (p/2)·m² - r·m + (pr/2 - q²/8) = 0
    let rc = vec![
        Value::float(p * r / 2.0 - q * q / 8.0),
        Value::float(-r),
        Value::float(-p / 2.0),
        Value::float(1.0),
    ];
    let m_roots = solve_cubic(&rc)?;

    // Pick a real m root such that 2m - p > 0 (for sqrt(2m-p) to be real).
    // If none exists, pick any real m (complex sqrt handles the rest).
    let m_f = m_roots
        .iter()
        .find_map(|rv| {
            let m = rv.as_f64()?;
            if 2.0 * m - p > 1e-12 { Some(m) } else { None }
        })
        .or_else(|| m_roots.iter().find_map(|rv| rv.as_f64()))
        .unwrap_or(1.0);

    // sqrt(2m - p)
    let two_m_minus_p = 2.0 * m_f - p;
    let sqrt_term = if two_m_minus_p >= 0.0 {
        Complex64::new(two_m_minus_p.sqrt(), 0.0)
    } else {
        Complex64::new(0.0, (-two_m_minus_p).sqrt())
    };

    let q_over_2sqrt = if sqrt_term.norm() > 1e-12 {
        Complex64::new(q, 0.0) / (2.0 * sqrt_term)
    } else {
        Complex64::new(0.0, 0.0)
    };

    let m_c = Complex64::new(m_f, 0.0);

    // Quadratic 1: t² + sqrt_term·t + (m - q_over_2sqrt) = 0
    let roots1 = solve_quadratic_c64(Complex64::new(1.0, 0.0), sqrt_term, m_c - q_over_2sqrt);

    // Quadratic 2: t² - sqrt_term·t + (m + q_over_2sqrt) = 0
    let roots2 = solve_quadratic_c64(Complex64::new(1.0, 0.0), -sqrt_term, m_c + q_over_2sqrt);

    // Undo depression: x = t - b/(4a)
    let mut all_roots = Vec::with_capacity(4);
    for t in roots1.iter().chain(roots2.iter()) {
        let x = t - Complex64::new(b_over_4, 0.0);
        all_roots.push(complex_to_value(x));
    }
    Ok(all_roots)
}

/// Solve a·t² + b·t + c = 0 with Complex64 coefficients, returning Complex64
/// roots.
#[allow(dead_code)]
fn solve_quadratic_c64(a: Complex64, b: Complex64, c: Complex64) -> Vec<Complex64> {
    if a.norm() < 1e-12 {
        return if b.norm() > 1e-12 {
            vec![-c / b]
        } else {
            vec![]
        };
    }
    let disc = b * b - 4.0 * a * c;
    let sqrt_disc = disc.sqrt();
    vec![(-b + sqrt_disc) / (2.0 * a), (-b - sqrt_disc) / (2.0 * a)]
}

fn linear_coefficients_from_expr(expr: &Value, vars: &[String]) -> WqResult<(Vec<Value>, Value)> {
    let expr = simplify_cas_value(expr)?;
    let terms = if let Some((CasOp::Add, args)) = expr.cas_op_parts() {
        args.to_vec()
    } else {
        vec![expr]
    };
    let mut coeffs = vec![Value::Int(0); vars.len()];
    let mut constant = Value::Int(0);

    for term in terms {
        let (coeff, core) = split_add_term(&term);
        match core {
            None => constant = eval_numeric_binary("+", &constant, &coeff)?,
            Some(core) => {
                let Some(name) = core.cas_var_name() else {
                    return Err(cas_err(
                        "solve_system currently supports linear equations in the requested variables only",
                    ));
                };
                let idx = vars.iter().position(|var| var == name).ok_or_else(|| {
                    cas_err(format!(
                        "solve_system encountered unknown variable '{name}'"
                    ))
                })?;
                coeffs[idx] = eval_numeric_binary("+", &coeffs[idx], &coeff)?;
            }
        }
    }

    Ok((coeffs, constant))
}

fn gaussian_elimination_solve(mut rows: Vec<Vec<Value>>) -> WqResult<Vec<Value>> {
    let n = rows.len();
    for col in 0..n {
        let Some(pivot_row) = (col..n).find(|&row| !numeric_is_zero(&rows[row][col])) else {
            return Err(cas_err(
                "solve_system requires a system with a unique solution",
            ));
        };
        if pivot_row != col {
            rows.swap(col, pivot_row);
        }

        let pivot = rows[col][col].clone();
        let pivot_slice = rows[col][col..=n].to_vec();
        let tail = &mut rows[(col + 1)..n];
        tail.par_iter_mut().try_for_each(|row| {
            if numeric_is_zero(&row[col]) {
                return Ok(());
            }
            let factor = eval_exact_numeric_div(&row[col], &pivot)?;
            for (offset, cell) in row[col..=n].iter_mut().enumerate() {
                let scaled = eval_numeric_binary("*", &factor, &pivot_slice[offset])?;
                *cell = eval_numeric_binary("-", cell, &scaled)?;
            }
            Ok(())
        })?;
    }

    let mut solution = vec![Value::Int(0); n];
    for row in (0..n).rev() {
        let mut rhs = rows[row][n].clone();
        for (col, value) in solution.iter().enumerate().skip(row + 1) {
            let term = eval_numeric_binary("*", &rows[row][col], value)?;
            rhs = eval_numeric_binary("-", &rhs, &term)?;
        }
        if numeric_is_zero(&rows[row][row]) {
            return Err(cas_err(
                "solve_system requires a system with a unique solution",
            ));
        }
        solution[row] = eval_exact_numeric_div(&rhs, &rows[row][row])?;
    }
    Ok(solution)
}

pub(crate) fn solve_cas(input: &Value, var: &Value) -> WqResult<Value> {
    let var = var_name_from_value(var)?;
    let expr = if let Some((lhs, rhs)) = input.cas_eq_parts() {
        cas_sub(lhs.clone(), rhs.clone())?
    } else {
        simplify_cas_value(input)?
    };
    let coeffs = poly_from_expr(&expr, &var)?;
    let degree = poly_degree(&coeffs);
    let roots = match degree {
        0 => Vec::new(),
        1 => vec![eval_exact_numeric_div(
            &coeffs[0].neg().map_err(|e| e.src("cas"))?,
            &coeffs[1],
        )?],
        2 => {
            let four_ac = eval_numeric_binary(
                "*",
                &Value::Int(4),
                &eval_numeric_binary("*", &coeffs[degree], &coeffs[0])?,
            )?;
            let disc = eval_numeric_binary(
                "-",
                &eval_numeric_binary("^", &coeffs[1], &Value::Int(2))?,
                &four_ac,
            )?;
            let sqrt_disc = disc.sqrt().map_err(|e| e.src("cas"))?;
            let neg_b = coeffs[1].neg().map_err(|e| e.src("cas"))?;
            let denom = eval_numeric_binary("*", &Value::Int(2), &coeffs[degree])?;
            vec![
                eval_numeric_binary("/", &eval_numeric_binary("+", &neg_b, &sqrt_disc)?, &denom)?,
                eval_numeric_binary("/", &eval_numeric_binary("-", &neg_b, &sqrt_disc)?, &denom)?,
            ]
        }
        _ => solve_monomial_polynomial(&coeffs, degree)?,
    };
    Ok(Value::List(Arc::new(roots)))
}

pub(crate) fn solve_system_cas(equations: &Value, vars: &Value) -> WqResult<Value> {
    let equations = match equations {
        Value::List(items) => items,
        _ => {
            return Err(
                cas_err("solve_system expects a list of equations or expressions").got1(equations),
            );
        }
    };
    let vars = match vars {
        Value::List(items) => items,
        _ => return Err(cas_err("solve_system expects a list of symbolic variables").got1(vars)),
    };
    if equations.len() != vars.len() {
        return Err(cas_err(
            "solve_system expects the same number of equations and variables",
        ));
    }

    let mut var_names = Vec::with_capacity(vars.len());
    for var in vars.iter() {
        var_names.push(var_name_from_value(var)?);
    }

    let mut rows = Vec::with_capacity(equations.len());
    for equation in equations.iter() {
        let expr = if let Some((lhs, rhs)) = equation.cas_eq_parts() {
            cas_sub(lhs.clone(), rhs.clone())?
        } else {
            simplify_cas_value(equation)?
        };
        let (coeffs, constant) = linear_coefficients_from_expr(&expr, &var_names)?;
        let mut row = coeffs;
        row.push(eval_numeric_binary("-", &Value::Int(0), &constant)?);
        rows.push(row);
    }

    Ok(Value::from_items(gaussian_elimination_solve(rows)?))
}
