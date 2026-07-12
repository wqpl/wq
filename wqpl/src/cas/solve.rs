use std::collections::BTreeSet;
use std::sync::Arc;

use indexmap::IndexMap;
use num_bigint::BigInt;
use num_complex::Complex64;
use rayon::prelude::*;

use super::assumption::{CasAssumptions, Truth};
use super::{
    cas_add, cas_div, cas_err, cas_mul, cas_neg, cas_pow, cas_sub, contains_cas_var,
    eval_exact_numeric_div, eval_numeric_binary, numeric_is_negative, numeric_is_zero, poly_degree,
    poly_from_expr, poly_from_expr_with_params, simplify_cas_value, var_name_from_value,
};
use crate::value::cas::CasOp;
use crate::value::{Value, WqResult, expected_numeric1};
use crate::wqerror::WqError;

fn complex_to_value(z: Complex64) -> Value {
    let eps = z.norm().max(1.0) * 1e-12;
    let re = if z.re.abs() <= eps { 0.0 } else { z.re };
    let im = if z.im.abs() <= eps { 0.0 } else { z.im };
    if im == 0.0 {
        Value::float(re)
    } else {
        Value::from_complex64(Complex64::new(re, im))
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
        .as_complex64()
        .ok_or_else(|| expected_numeric1(&coeffs[degree]).src("cas"))?;
    if leading == Complex64::new(0.0, 0.0) {
        return Err(cas_err("leading coefficient cannot be zero"));
    }
    let constant = coeffs[0]
        .as_complex64()
        .ok_or_else(|| expected_numeric1(&coeffs[0]).src("cas"))?;
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
        match split_linear_term(&term, vars)? {
            LinearTerm::Constant(value) => constant = cas_add(vec![constant, value])?,
            LinearTerm::Variable { idx, coeff } => {
                coeffs[idx] = cas_add(vec![coeffs[idx].clone(), coeff])?;
            }
        }
    }

    Ok((coeffs, constant))
}

enum LinearTerm {
    Constant(Value),
    Variable { idx: usize, coeff: Value },
}

fn split_linear_term(term: &Value, vars: &[String]) -> WqResult<LinearTerm> {
    if !contains_requested_var(term, vars) {
        return Ok(LinearTerm::Constant(term.clone()));
    }

    if let Some(idx) = requested_var_index(term, vars) {
        return Ok(LinearTerm::Variable {
            idx,
            coeff: Value::Int(1),
        });
    }

    if let Some((CasOp::Multiply, args)) = term.cas_op_parts() {
        let mut var_idx = None;
        let mut coeff_factors = Vec::new();
        for arg in args {
            if !contains_requested_var(arg, vars) {
                coeff_factors.push(arg.clone());
                continue;
            }

            let Some(idx) = requested_var_index(arg, vars) else {
                return Err(linear_system_shape_err());
            };
            if var_idx.replace(idx).is_some() {
                return Err(linear_system_shape_err());
            }
        }

        let Some(idx) = var_idx else {
            return Ok(LinearTerm::Constant(term.clone()));
        };
        let coeff = match coeff_factors.len() {
            0 => Value::Int(1),
            1 => coeff_factors
                .into_iter()
                .next()
                .expect("single coefficient factor"),
            _ => cas_mul(coeff_factors)?,
        };
        return Ok(LinearTerm::Variable { idx, coeff });
    }

    Err(linear_system_shape_err())
}

fn contains_requested_var(expr: &Value, vars: &[String]) -> bool {
    vars.iter().any(|var| contains_cas_var(expr, var))
}

fn requested_var_index(expr: &Value, vars: &[String]) -> Option<usize> {
    let name = expr.cas_var_name()?;
    vars.iter().position(|var| var == name)
}

fn linear_system_shape_err() -> WqError {
    cas_err("solve_system currently supports linear equations in the requested variables only")
}

fn undecidable_zero_error(value: &Value, role: &str) -> WqError {
    cas_err(format!(
        "solve_system cannot determine whether {role} {value} is zero; pass nonzero[{value}] to named argument assuming, or assert zero with eq[{value};0]"
    ))
}

fn pivot_row_for_col(
    rows: &[Vec<Value>],
    start_row: usize,
    col: usize,
    assumptions: &CasAssumptions,
) -> WqResult<Option<usize>> {
    if let Some(row) = (start_row..rows.len()).find(|&row| {
        (rows[row][col].exact_int_is(1) || rows[row][col].exact_int_is(-1))
            && assumptions.prove_zero(&rows[row][col]) == Truth::Refuted
    }) {
        return Ok(Some(row));
    }
    if let Some(row) = (start_row..rows.len())
        .find(|&row| assumptions.prove_zero(&rows[row][col]) == Truth::Refuted)
    {
        return Ok(Some(row));
    }
    if let Some(row) = (start_row..rows.len())
        .find(|&row| assumptions.prove_zero(&rows[row][col]) == Truth::Unknown)
    {
        return Err(undecidable_zero_error(&rows[row][col], "pivot candidate"));
    }
    Ok(None)
}

fn row_has_zero_coefficients(
    row: &[Value],
    var_count: usize,
    assumptions: &CasAssumptions,
) -> WqResult<bool> {
    for coefficient in &row[..var_count] {
        match assumptions.prove_zero(coefficient) {
            Truth::Proven => {}
            Truth::Refuted => return Ok(false),
            Truth::Unknown => {
                return Err(undecidable_zero_error(coefficient, "row coefficient"));
            }
        }
    }
    Ok(true)
}

fn gaussian_elimination_solve(
    mut rows: Vec<Vec<Value>>,
    var_count: usize,
    assumptions: &CasAssumptions,
) -> WqResult<Vec<Value>> {
    let rhs_col = var_count;
    let mut rank = 0;
    let mut pivot_cols = Vec::with_capacity(var_count);

    for col in 0..var_count {
        let Some(pivot_row) = pivot_row_for_col(&rows, rank, col, assumptions)? else {
            continue;
        };
        if pivot_row != rank {
            rows.swap(rank, pivot_row);
        }

        let pivot = rows[rank][col].clone();
        let pivot_slice = rows[rank][col..=rhs_col].to_vec();
        for row in &mut rows[(rank + 1)..] {
            match assumptions.prove_zero(&row[col]) {
                Truth::Proven => continue,
                Truth::Refuted => {}
                Truth::Unknown => {
                    return Err(undecidable_zero_error(&row[col], "elimination coefficient"));
                }
            }
            let factor = cas_div(row[col].clone(), pivot.clone())?;
            for (offset, cell) in row[col..=rhs_col].iter_mut().enumerate() {
                let scaled = cas_mul(vec![factor.clone(), pivot_slice[offset].clone()])?;
                *cell = cas_sub(cell.clone(), scaled)?;
            }
        }

        pivot_cols.push(col);
        rank += 1;
    }

    for row in &rows {
        if row_has_zero_coefficients(row, var_count, assumptions)? {
            match assumptions.prove_zero(&row[rhs_col]) {
                Truth::Proven => {}
                Truth::Refuted => {
                    return Err(cas_err(
                        "solve_system has no solution (inconsistent system)",
                    ));
                }
                Truth::Unknown => {
                    return Err(undecidable_zero_error(&row[rhs_col], "residual"));
                }
            }
        }
    }

    if rank < var_count {
        return Err(cas_err(
            "solve_system has infinitely many solutions (dependent system)",
        ));
    }

    let mut solution = vec![Value::Int(0); var_count];
    for pivot_idx in (0..rank).rev() {
        let row_idx = pivot_idx;
        let pivot_col = pivot_cols[pivot_idx];
        let mut rhs = rows[row_idx][rhs_col].clone();
        for (col, value) in solution.iter().enumerate().skip(pivot_col + 1) {
            let term = cas_mul(vec![rows[row_idx][col].clone(), value.clone()])?;
            rhs = cas_sub(rhs, term)?;
        }
        solution[pivot_col] = cas_div(rhs, rows[row_idx][pivot_col].clone())?;
    }
    Ok(solution)
}

fn determinant(matrix: &[Vec<Value>]) -> WqResult<Value> {
    match matrix.len() {
        0 => Ok(Value::Int(1)),
        1 => Ok(matrix[0][0].clone()),
        size => {
            let mut terms = Vec::with_capacity(size);
            for col in 0..size {
                let mut minor = Vec::with_capacity(size - 1);
                for row in matrix.iter().skip(1) {
                    let mut minor_row = Vec::with_capacity(size - 1);
                    for (idx, value) in row.iter().enumerate() {
                        if idx != col {
                            minor_row.push(value.clone());
                        }
                    }
                    minor.push(minor_row);
                }
                let term = cas_mul(vec![matrix[0][col].clone(), determinant(&minor)?])?;
                terms.push(if col % 2 == 0 { term } else { cas_neg(term)? });
            }
            cas_add(terms)
        }
    }
}

fn solve_square_system(
    rows: &[Vec<Value>],
    var_count: usize,
    assumptions: &CasAssumptions,
) -> WqResult<Option<Vec<Value>>> {
    if rows.len() != var_count || var_count == 0 || var_count > 6 {
        return Ok(None);
    }
    let matrix = rows
        .iter()
        .map(|row| row[..var_count].to_vec())
        .collect::<Vec<_>>();
    if !matrix.iter().flatten().any(Value::is_cas_expr) {
        return Ok(None);
    }
    let det = determinant(&matrix)?;
    match assumptions.prove_zero(&det) {
        Truth::Proven => Ok(None),
        Truth::Unknown => Err(undecidable_zero_error(&det, "determinant")),
        Truth::Refuted => {
            let mut solution = Vec::with_capacity(var_count);
            for replaced_col in 0..var_count {
                let mut replaced = matrix.clone();
                for (row_idx, row) in rows.iter().enumerate() {
                    replaced[row_idx][replaced_col] = row[var_count].clone();
                }
                solution.push(cas_div(determinant(&replaced)?, det.clone())?);
            }
            Ok(Some(solution))
        }
    }
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
    let mut seen = BTreeSet::new();
    for var in vars.iter() {
        let name = var_name_from_value(var)?;
        if !seen.insert(name.clone()) {
            return Err(cas_err(format!(
                "solve_system variable '{name}' appears more than once"
            )));
        }
        var_names.push(name);
    }
    Ok(var_names)
}

fn solve_normalized_system(
    equations: &[Value],
    var_names: &[String],
    assumptions: &CasAssumptions,
) -> WqResult<Value> {
    let mut rows = Vec::with_capacity(equations.len());
    for expr in equations {
        let (coeffs, constant) = linear_coefficients_from_expr(expr, var_names)?;
        let mut row = coeffs;
        row.push(cas_neg(constant)?);
        rows.push(row);
    }

    let solution = if let Some(solution) = solve_square_system(&rows, var_names.len(), assumptions)?
    {
        solution
    } else {
        gaussian_elimination_solve(rows, var_names.len(), assumptions)?
    };
    let mut map = IndexMap::with_capacity(solution.len());
    for (var_name, value) in var_names.iter().zip(solution) {
        map.insert(Arc::from(var_name.as_str()), value);
    }
    Ok(Value::Dict(Arc::new(map)))
}

fn coefficients_are_exact_real(coeffs: &[Value]) -> bool {
    coeffs
        .iter()
        .all(|coeff| !matches!(coeff, Value::Float(_) | Value::Complex(_)))
}

fn quadratic_discriminant(coeffs: &[Value]) -> WqResult<Value> {
    let four_ac = eval_numeric_binary(
        "*",
        &Value::Int(4),
        &eval_numeric_binary("*", &coeffs[2], &coeffs[0])?,
    )?;
    eval_numeric_binary(
        "-",
        &eval_numeric_binary("^", &coeffs[1], &Value::Int(2))?,
        &four_ac,
    )
}

fn solve_numeric_quadratic(coeffs: &[Value], disc: Value) -> WqResult<Vec<Value>> {
    let sqrt_disc = disc.sqrt().map_err(|e| e.src("cas"))?;
    let neg_b = coeffs[1].neg().map_err(|e| e.src("cas"))?;
    let denom = eval_numeric_binary("*", &Value::Int(2), &coeffs[2])?;
    Ok(vec![
        eval_numeric_binary("/", &eval_numeric_binary("+", &neg_b, &sqrt_disc)?, &denom)?,
        eval_numeric_binary("/", &eval_numeric_binary("-", &neg_b, &sqrt_disc)?, &denom)?,
    ])
}

fn solve_numeric_polynomial(coeffs: &[Value]) -> WqResult<Vec<Value>> {
    let degree = poly_degree(coeffs);
    match degree {
        0 => {
            if numeric_is_zero(&coeffs[0]) {
                return Err(cas_err("solve identity has infinitely many solutions"));
            }
            Ok(Vec::new())
        }
        1 => Ok(vec![eval_exact_numeric_div(
            &coeffs[0].neg().map_err(|e| e.src("cas"))?,
            &coeffs[1],
        )?]),
        2 => {
            let disc = quadratic_discriminant(coeffs)?;
            if coefficients_are_exact_real(coeffs) && !numeric_is_negative(&disc) {
                solve_parameterized_polynomial(coeffs, &CasAssumptions::default())
            } else {
                solve_numeric_quadratic(coeffs, disc)
            }
        }
        _ => solve_monomial_polynomial(coeffs, degree),
    }
}

fn parameterized_poly_degree(coeffs: &[Value], assumptions: &CasAssumptions) -> WqResult<usize> {
    for (degree, coefficient) in coeffs.iter().enumerate().rev() {
        match assumptions.prove_zero(coefficient) {
            Truth::Proven => {}
            Truth::Refuted => return Ok(degree),
            Truth::Unknown => {
                return Err(cas_err(format!(
                    "solve cannot determine whether leading coefficient {coefficient} is zero; pass nonzero[{coefficient}] to named argument assuming, or assert zero with eq[{coefficient};0]"
                )));
            }
        }
    }
    Ok(0)
}

fn solve_parameterized_polynomial(
    coeffs: &[Value],
    assumptions: &CasAssumptions,
) -> WqResult<Vec<Value>> {
    let coeff_at = |idx: usize| coeffs.get(idx).cloned().unwrap_or(Value::Int(0));
    let degree = parameterized_poly_degree(coeffs, assumptions)?;
    match degree {
        0 => match assumptions.prove_zero(&coeffs[0]) {
            Truth::Proven => Err(cas_err("solve identity has infinitely many solutions")),
            Truth::Refuted => Ok(Vec::new()),
            Truth::Unknown => Err(cas_err(format!(
                "solve cannot determine whether constant {} is zero; pass nonzero[{}] to named argument assuming, or assert zero with eq[{};0]",
                coeffs[0], coeffs[0], coeffs[0]
            ))),
        },
        1 => {
            let root = cas_div(cas_neg(coeff_at(0))?, coeff_at(1))?;
            Ok(vec![simplify_cas_value(&root)?])
        }
        2 => {
            let a = coeff_at(2);
            let b = coeff_at(1);
            let c = coeff_at(0);
            let b_squared = cas_pow(b.clone(), Value::Int(2))?;
            let four_ac = cas_mul(vec![Value::Int(4), a.clone(), c])?;
            let disc = cas_sub(b_squared, four_ac)?;
            let sqrt_disc = cas_pow(
                disc,
                Value::from_fraction_parts(BigInt::from(1), BigInt::from(2)),
            )?;
            let neg_b = cas_neg(b)?;
            let denom = cas_mul(vec![Value::Int(2), a])?;
            Ok(vec![
                simplify_cas_value(&cas_div(
                    cas_add(vec![neg_b.clone(), sqrt_disc.clone()])?,
                    denom.clone(),
                )?)?,
                simplify_cas_value(&cas_div(cas_sub(neg_b, sqrt_disc)?, denom)?)?,
            ])
        }
        _ => Err(cas_err(
            "parameterized solve currently supports polynomial degree <= 2",
        )),
    }
}

pub(crate) fn solve_cas(input: &Value, var: &Value) -> WqResult<Value> {
    solve_cas_with_assumptions(input, var, &CasAssumptions::default())
}

pub(crate) fn solve_cas_with_assumptions(
    input: &Value,
    var: &Value,
    assumptions: &CasAssumptions,
) -> WqResult<Value> {
    let var = var_name_from_value(var)?;
    let expr = if let Some((lhs, rhs)) = input.cas_eq_parts() {
        cas_sub(lhs.clone(), rhs.clone())?
    } else {
        simplify_cas_value(input)?
    };
    let roots = match poly_from_expr(&expr, &var) {
        Ok(coeffs) => solve_numeric_polynomial(&coeffs)?,
        Err(_) => {
            let coeffs = poly_from_expr_with_params(&expr, &var)?;
            solve_parameterized_polynomial(&coeffs, assumptions)?
        }
    };
    Ok(Value::List(Arc::new(roots)))
}

#[cfg(test)]
pub(crate) fn solve_system_cas(equations: &Value, vars: &Value) -> WqResult<Value> {
    solve_system_cas_with_assumptions(equations, vars, &CasAssumptions::default())
}

pub(crate) fn solve_system_cas_with_assumptions(
    equations: &Value,
    vars: &Value,
    assumptions: &CasAssumptions,
) -> WqResult<Value> {
    let equations = normalize_system_equations(equations)?;
    let var_names = parse_system_var_names(vars)?;
    if var_names.is_empty() {
        return Err(cas_err(
            "solve_system expects at least one symbolic variable",
        ));
    }

    solve_normalized_system(&equations, &var_names, assumptions)
}

#[cfg(test)]
pub(crate) fn solve_system_infer_cas(equations: &Value) -> WqResult<Value> {
    solve_system_infer_cas_with_assumptions(equations, &CasAssumptions::default())
}

pub(crate) fn solve_system_infer_cas_with_assumptions(
    equations: &Value,
    assumptions: &CasAssumptions,
) -> WqResult<Value> {
    let equations = normalize_system_equations(equations)?;
    let var_names = infer_system_var_names(&equations)?;

    solve_normalized_system(&equations, &var_names, assumptions)
}
