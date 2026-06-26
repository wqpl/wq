use std::collections::BTreeSet;
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

fn normalize_system_equation(equation: &Value) -> WqResult<Value> {
    if let Some((lhs, rhs)) = equation.cas_eq_parts() {
        cas_sub(lhs.clone(), rhs.clone())
    } else {
        simplify_cas_value(equation)
    }
}

fn normalize_system_equations(equations: &Value) -> WqResult<Vec<Value>> {
    let equations = match equations {
        Value::List(items) => items,
        _ => {
            return Err(
                cas_err("solve_system expects a list of equations or expressions").got1(equations),
            );
        }
    };

    equations.iter().map(normalize_system_equation).collect()
}

fn collect_cas_vars(expr: &Value, vars: &mut BTreeSet<String>) {
    if let Some(name) = expr.cas_var_name() {
        vars.insert(name.to_string());
        return;
    }
    if let Some((_, args)) = expr.cas_op_parts() {
        for arg in args {
            collect_cas_vars(arg, vars);
        }
    }
    if let Some((_, args)) = expr.cas_function_parts() {
        for arg in args {
            collect_cas_vars(arg, vars);
        }
    }
    if let Some((_, args)) = expr.cas_apply_parts() {
        for arg in args {
            collect_cas_vars(arg, vars);
        }
    }
    if let Some((_name, value)) = expr.cas_named_arg_parts() {
        collect_cas_vars(value, vars);
    }
    if let Some((inner, limit_var, point, _direction)) = expr.cas_limit_parts() {
        collect_cas_vars(inner, vars);
        collect_cas_vars(limit_var, vars);
        collect_cas_vars(point, vars);
    }
    if let Some((lhs, rhs)) = expr.cas_eq_parts() {
        collect_cas_vars(lhs, vars);
        collect_cas_vars(rhs, vars);
    }
    if let Value::List(items) = expr {
        for item in items.iter() {
            collect_cas_vars(item, vars);
        }
    }
}

fn infer_system_var_names(equations: &[Value]) -> WqResult<Vec<String>> {
    let mut vars = BTreeSet::new();
    for equation in equations {
        collect_cas_vars(equation, &mut vars);
    }
    if vars.is_empty() {
        return Err(cas_err("solve_system could not infer symbolic variables"));
    }
    Ok(vars.into_iter().collect())
}

fn parse_system_var_names(vars: &Value) -> WqResult<Vec<String>> {
    let vars = match vars {
        Value::List(items) => items,
        _ => return Err(cas_err("solve_system expects a list of symbolic variables").got1(vars)),
    };
    let mut var_names = Vec::with_capacity(vars.len());
    for var in vars.iter() {
        var_names.push(var_name_from_value(var)?);
    }
    Ok(var_names)
}

fn solve_normalized_system(equations: &[Value], var_names: &[String]) -> WqResult<Value> {
    let mut rows = Vec::with_capacity(equations.len());
    for expr in equations {
        let (coeffs, constant) = linear_coefficients_from_expr(expr, var_names)?;
        let mut row = coeffs;
        row.push(eval_numeric_binary("-", &Value::Int(0), &constant)?);
        rows.push(row);
    }

    Ok(Value::from_items(gaussian_elimination_solve(rows)?))
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
    let equations = normalize_system_equations(equations)?;
    let var_names = parse_system_var_names(vars)?;
    if equations.len() != var_names.len() {
        return Err(cas_err(
            "solve_system expects the same number of equations and variables",
        ));
    }

    solve_normalized_system(&equations, &var_names)
}

pub(crate) fn solve_system_infer_cas(equations: &Value) -> WqResult<Value> {
    let equations = normalize_system_equations(equations)?;
    let var_names = infer_system_var_names(&equations)?;
    if equations.len() != var_names.len() {
        return Err(cas_err(format!(
            "solve_system inferred {} variables for {} equations; pass an explicit variable list",
            var_names.len(),
            equations.len()
        )));
    }

    solve_normalized_system(&equations, &var_names)
}
