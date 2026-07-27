use std::collections::{BTreeSet, HashMap};
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
use crate::value::cas::{CasConst, CasFunction, CasOp, CasPredicate};
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
            "'solve' currently supports degree {degree} only for equations of the form a*x^{degree} + b = 0"
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

fn solve_exact_monomial_polynomial(
    coeffs: &[Value],
    degree: usize,
    domain: SolveDomain,
) -> WqResult<Option<Vec<Value>>> {
    if !coefficients_are_exact_real(coeffs)
        || degree >= coeffs.len()
        || coeffs[1..degree]
            .iter()
            .any(|coeff| !numeric_is_zero(coeff))
    {
        return Ok(None);
    }

    let target = eval_exact_numeric_div(
        &coeffs[0].neg().map_err(|error| error.src("cas"))?,
        &coeffs[degree],
    )?;
    if numeric_is_zero(&target) {
        return Ok(Some(vec![Value::Int(0)]));
    }

    let negative = numeric_is_negative(&target);
    let magnitude = if negative { cas_neg(target)? } else { target };
    let degree_value = BigInt::from(degree);
    let radius = cas_pow(
        magnitude,
        Value::from_fraction_parts(BigInt::from(1), degree_value.clone()),
    )?;

    if domain == SolveDomain::Real {
        if negative && degree.is_multiple_of(2) {
            return Ok(Some(Vec::new()));
        }
        let principal = if negative {
            cas_neg(radius.clone())?
        } else {
            radius.clone()
        };
        if !negative && degree.is_multiple_of(2) {
            return Ok(Some(vec![principal, cas_neg(radius)?]));
        }
        return Ok(Some(vec![principal]));
    }

    let imaginary_unit = Value::from_cas_op(
        CasOp::Power,
        vec![
            Value::Int(-1),
            Value::from_fraction_parts(BigInt::from(1), BigInt::from(2)),
        ],
    );
    let mut roots = Vec::with_capacity(degree);
    for index in 0..degree {
        let angle_numerator = if negative { 2 * index + 1 } else { 2 * index };
        let angle = cas_mul(vec![
            Value::from_fraction_parts(BigInt::from(angle_numerator), degree_value.clone()),
            Value::from_cas_const(CasConst::Pi),
        ])?;
        let cosine = simplify_cas_value(&Value::from_cas_function(
            CasFunction::Cos,
            vec![angle.clone()],
        ))?;
        let sine = simplify_cas_value(&Value::from_cas_function(CasFunction::Sin, vec![angle]))?;
        let unit = cas_add(vec![cosine, cas_mul(vec![imaginary_unit.clone(), sine])?])?;
        roots.push(simplify_cas_value(&cas_mul(vec![radius.clone(), unit])?)?);
    }
    Ok(Some(roots))
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
    cas_err("'solve_system' currently supports linear equations in the requested variables only")
}

#[derive(Clone)]
struct ConditionalCase<T> {
    conditions: Vec<CasPredicate>,
    result: T,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SolveDomain {
    Complex,
    Real,
}

#[derive(Clone)]
enum RootSolution {
    Empty,
    All,
    Finite(Vec<Value>),
    Cases(Vec<ConditionalCase<RootSolution>>),
}

enum DegreeDecision {
    Degree(usize),
    NeedDecision(Value),
}

#[derive(Clone)]
enum LinearSolution {
    Empty,
    Unique(Vec<Value>),
    Parametric {
        values: Vec<Value>,
        parameters: Vec<Value>,
    },
    Cases(Vec<ConditionalCase<LinearSolution>>),
}

enum PivotDecision {
    Row(usize),
    None,
    NeedDecision(Value),
}

enum GaussianOutcome {
    Solved(LinearSolution),
    NeedDecision(Value),
}

enum SquareOutcome {
    NotApplicable,
    Solved(Vec<Value>),
    NeedDecision(Value),
}

fn pivot_row_for_col(
    rows: &[Vec<Value>],
    start_row: usize,
    col: usize,
    assumptions: &CasAssumptions,
) -> PivotDecision {
    if let Some(row) = (start_row..rows.len()).find(|&row| {
        (rows[row][col].exact_int_is(1) || rows[row][col].exact_int_is(-1))
            && assumptions.prove_zero(&rows[row][col]) == Truth::Refuted
    }) {
        return PivotDecision::Row(row);
    }
    if let Some(row) = (start_row..rows.len())
        .find(|&row| assumptions.prove_zero(&rows[row][col]) == Truth::Refuted)
    {
        return PivotDecision::Row(row);
    }
    if let Some(row) = (start_row..rows.len())
        .find(|&row| assumptions.prove_zero(&rows[row][col]) == Truth::Unknown)
    {
        return PivotDecision::NeedDecision(rows[row][col].clone());
    }
    PivotDecision::None
}

fn row_has_zero_coefficients(
    row: &[Value],
    var_count: usize,
    assumptions: &CasAssumptions,
) -> Result<bool, Value> {
    let mut unknown = None;
    for coefficient in &row[..var_count] {
        match assumptions.prove_zero(coefficient) {
            Truth::Proven => {}
            Truth::Refuted => return Ok(false),
            Truth::Unknown => {
                unknown.get_or_insert_with(|| coefficient.clone());
            }
        }
    }
    if let Some(unknown) = unknown {
        Err(unknown)
    } else {
        Ok(true)
    }
}

fn gaussian_elimination(
    mut rows: Vec<Vec<Value>>,
    var_count: usize,
    var_names: &[String],
    assumptions: &CasAssumptions,
) -> WqResult<GaussianOutcome> {
    let rhs_col = var_count;
    let mut rank = 0;
    let mut pivot_cols = Vec::with_capacity(var_count);

    for col in 0..var_count {
        let pivot_row = match pivot_row_for_col(&rows, rank, col, assumptions) {
            PivotDecision::Row(row) => row,
            PivotDecision::None => continue,
            PivotDecision::NeedDecision(value) => {
                return Ok(GaussianOutcome::NeedDecision(value));
            }
        };
        if pivot_row != rank {
            rows.swap(rank, pivot_row);
        }

        let pivot = rows[rank][col].clone();
        let pivot_slice = rows[rank][col..=rhs_col].to_vec();
        for row in &mut rows[(rank + 1)..] {
            if assumptions.prove_zero(&row[col]) == Truth::Proven {
                continue;
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
        let all_zero = match row_has_zero_coefficients(row, var_count, assumptions) {
            Ok(all_zero) => all_zero,
            Err(value) => return Ok(GaussianOutcome::NeedDecision(value)),
        };
        if all_zero {
            match assumptions.prove_zero(&row[rhs_col]) {
                Truth::Proven => {}
                Truth::Refuted => {
                    return Ok(GaussianOutcome::Solved(LinearSolution::Empty));
                }
                Truth::Unknown => {
                    return Ok(GaussianOutcome::NeedDecision(row[rhs_col].clone()));
                }
            }
        }
    }

    let free_cols = (0..var_count)
        .filter(|col| !pivot_cols.contains(col))
        .collect::<Vec<_>>();
    let parameters = fresh_parameters(&rows, var_names, free_cols.len());
    let mut solution = vec![Value::Int(0); var_count];
    for (&col, parameter) in free_cols.iter().zip(&parameters) {
        solution[col] = parameter.clone();
    }

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
    if parameters.is_empty() {
        Ok(GaussianOutcome::Solved(LinearSolution::Unique(solution)))
    } else {
        Ok(GaussianOutcome::Solved(LinearSolution::Parametric {
            values: solution,
            parameters,
        }))
    }
}

fn fresh_parameters(rows: &[Vec<Value>], var_names: &[String], count: usize) -> Vec<Value> {
    let mut used = var_names.iter().cloned().collect::<BTreeSet<_>>();
    for row in rows {
        for value in row {
            collect_cas_vars(value, &mut used);
        }
    }
    let mut parameters = Vec::with_capacity(count);
    let mut index = 0usize;
    while parameters.len() < count {
        let name = format!("p{index}");
        if used.insert(name.clone()) {
            parameters.push(Value::from_cas_var(name));
        }
        index += 1;
    }
    parameters
}

fn determinant(matrix: &[Vec<Value>]) -> WqResult<Value> {
    if matrix.is_empty() {
        return Ok(Value::Int(1));
    }
    if matrix.len() > 12 {
        return Err(cas_err(
            "symbolic determinant currently supports matrices up to 12 by 12",
        ));
    }
    let full_mask = (1u16 << matrix.len()) - 1;
    determinant_mask(matrix, 0, full_mask, &mut HashMap::new())
}

fn determinant_mask(
    matrix: &[Vec<Value>],
    row: usize,
    columns: u16,
    memo: &mut HashMap<(usize, u16), Value>,
) -> WqResult<Value> {
    if row == matrix.len() {
        return Ok(Value::Int(1));
    }
    if let Some(value) = memo.get(&(row, columns)) {
        return Ok(value.clone());
    }

    let mut terms = Vec::with_capacity(columns.count_ones() as usize);
    let mut position = 0usize;
    for col in 0..matrix.len() {
        let bit = 1u16 << col;
        if columns & bit == 0 {
            continue;
        }
        let minor = determinant_mask(matrix, row + 1, columns ^ bit, memo)?;
        let term = cas_mul(vec![matrix[row][col].clone(), minor])?;
        terms.push(if position.is_multiple_of(2) {
            term
        } else {
            cas_neg(term)?
        });
        position += 1;
    }
    let result = cas_add(terms)?;
    memo.insert((row, columns), result.clone());
    Ok(result)
}

fn solve_square_system(
    rows: &[Vec<Value>],
    var_count: usize,
    assumptions: &CasAssumptions,
) -> WqResult<SquareOutcome> {
    if rows.len() != var_count || var_count == 0 || var_count > 12 {
        return Ok(SquareOutcome::NotApplicable);
    }
    let matrix = rows
        .iter()
        .map(|row| row[..var_count].to_vec())
        .collect::<Vec<_>>();
    if !matrix.iter().flatten().any(Value::is_cas_expr) {
        return Ok(SquareOutcome::NotApplicable);
    }
    let det = determinant(&matrix)?;
    match assumptions.prove_zero(&det) {
        Truth::Proven => Ok(SquareOutcome::NotApplicable),
        Truth::Unknown => Ok(SquareOutcome::NeedDecision(det)),
        Truth::Refuted => {
            let mut solution = Vec::with_capacity(var_count);
            for replaced_col in 0..var_count {
                let mut replaced = matrix.clone();
                for (row_idx, row) in rows.iter().enumerate() {
                    replaced[row_idx][replaced_col] = row[var_count].clone();
                }
                solution.push(cas_div(determinant(&replaced)?, det.clone())?);
            }
            Ok(SquareOutcome::Solved(solution))
        }
    }
}

fn solve_linear_with_cases(
    rows: &[Vec<Value>],
    var_names: &[String],
    assumptions: &CasAssumptions,
    branch_depth: usize,
) -> WqResult<LinearSolution> {
    match solve_square_system(rows, var_names.len(), assumptions)? {
        SquareOutcome::Solved(values) => return Ok(LinearSolution::Unique(values)),
        SquareOutcome::NeedDecision(value) => {
            return branch_linear_solution(rows, var_names, assumptions, value, branch_depth);
        }
        SquareOutcome::NotApplicable => {}
    }
    match gaussian_elimination(rows.to_vec(), var_names.len(), var_names, assumptions)? {
        GaussianOutcome::Solved(solution) => Ok(solution),
        GaussianOutcome::NeedDecision(value) => {
            branch_linear_solution(rows, var_names, assumptions, value, branch_depth)
        }
    }
}

fn branch_linear_solution(
    rows: &[Vec<Value>],
    var_names: &[String],
    assumptions: &CasAssumptions,
    value: Value,
    branch_depth: usize,
) -> WqResult<LinearSolution> {
    if branch_depth == 0 {
        return Err(cas_err(
            "'solve_system' exceeded its conditional branch limit; pass more assumptions",
        ));
    }
    let predicates = [
        CasPredicate::NonZero(value.clone()),
        CasPredicate::Zero(value),
    ];
    let mut cases = Vec::with_capacity(predicates.len());
    for predicate in predicates {
        let branch_assumptions = assumptions.clone().with_predicate(predicate.clone())?;
        let result =
            solve_linear_with_cases(rows, var_names, &branch_assumptions, branch_depth - 1)?;
        push_linear_case(&mut cases, predicate, result);
    }
    Ok(LinearSolution::Cases(cases))
}

fn push_linear_case(
    cases: &mut Vec<ConditionalCase<LinearSolution>>,
    predicate: CasPredicate,
    result: LinearSolution,
) {
    match result {
        LinearSolution::Cases(nested) => {
            for mut case in nested {
                case.conditions.insert(0, predicate.clone());
                cases.push(case);
            }
        }
        result => cases.push(ConditionalCase {
            conditions: vec![predicate],
            result,
        }),
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
                cas_err("'solve_system' expects a list of equations or expressions")
                    .got1(equations),
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
    if let Some((scope, bounds)) = expr.cas_integral_parts() {
        collect_cas_vars(scope.body(), vars);
        if let Some((lower, upper)) = bounds {
            collect_cas_vars(lower, vars);
            collect_cas_vars(upper, vars);
        }
    }
    if let Some((scope, point, _direction)) = expr.cas_limit_parts() {
        collect_cas_vars(scope.body(), vars);
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
        return Err(cas_err("'solve_system' could not infer symbolic variables"));
    }
    Ok(vars.into_iter().collect())
}

fn parse_system_var_names(vars: &Value) -> WqResult<Vec<String>> {
    let vars = match vars {
        Value::List(items) => items,
        _ => {
            return Err(cas_err("'solve_system' expects a list of symbolic variables").got1(vars));
        }
    };
    let mut var_names = Vec::with_capacity(vars.len());
    let mut seen = BTreeSet::new();
    for var in vars.iter() {
        let name = var_name_from_value(var)?;
        if !seen.insert(name.clone()) {
            return Err(cas_err(format!(
                "'solve_system' variable '{name}' appears more than once"
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

    let solution = solve_linear_with_cases(&rows, var_names, assumptions, 8)?;
    Ok(linear_solution_value(solution, var_names))
}

fn binding_dict(var_names: &[String], values: Vec<Value>) -> Value {
    let mut map = IndexMap::with_capacity(values.len());
    for (var_name, value) in var_names.iter().zip(values) {
        map.insert(Arc::from(var_name.as_str()), value);
    }
    Value::Dict(Arc::new(map))
}

fn named_dict(entries: impl IntoIterator<Item = (&'static str, Value)>) -> Value {
    Value::Dict(Arc::new(
        entries
            .into_iter()
            .map(|(name, value)| (Arc::from(name), value))
            .collect(),
    ))
}

fn condition_value(conditions: Vec<CasPredicate>) -> Value {
    Value::List(Arc::new(
        conditions
            .into_iter()
            .map(Value::from_cas_predicate)
            .collect(),
    ))
}

fn linear_solution_value(solution: LinearSolution, var_names: &[String]) -> Value {
    match solution {
        LinearSolution::Empty => Value::Tag(Arc::from("none")),
        LinearSolution::Unique(values) => binding_dict(var_names, values),
        LinearSolution::Parametric { values, parameters } => named_dict([
            ("solution", binding_dict(var_names, values)),
            ("parameters", Value::List(Arc::new(parameters))),
        ]),
        LinearSolution::Cases(cases) => named_dict([(
            "cases",
            Value::List(Arc::new(
                cases
                    .into_iter()
                    .map(|case| {
                        named_dict([
                            ("when", condition_value(case.conditions)),
                            ("solution", linear_solution_value(case.result, var_names)),
                        ])
                    })
                    .collect(),
            )),
        )]),
    }
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

fn solve_numeric_quadratic(
    coeffs: &[Value],
    disc: Value,
    domain: SolveDomain,
) -> WqResult<RootSolution> {
    if domain == SolveDomain::Real && numeric_is_negative(&disc) {
        return Ok(RootSolution::Empty);
    }
    let sqrt_disc = disc.sqrt().map_err(|e| e.src("cas"))?;
    let neg_b = coeffs[1].neg().map_err(|e| e.src("cas"))?;
    let denom = eval_numeric_binary("*", &Value::Int(2), &coeffs[2])?;
    let first = eval_numeric_binary("/", &eval_numeric_binary("+", &neg_b, &sqrt_disc)?, &denom)?;
    if domain == SolveDomain::Real && numeric_is_zero(&disc) {
        Ok(RootSolution::Finite(vec![first]))
    } else {
        Ok(RootSolution::Finite(vec![
            first,
            eval_numeric_binary("/", &eval_numeric_binary("-", &neg_b, &sqrt_disc)?, &denom)?,
        ]))
    }
}

fn solve_exact_complex_quadratic(coeffs: &[Value], disc: Value) -> WqResult<RootSolution> {
    let sqrt_disc = simplify_cas_value(&Value::from_cas_op(
        CasOp::Power,
        vec![
            disc,
            Value::from_fraction_parts(BigInt::from(1), BigInt::from(2)),
        ],
    ))?;
    let neg_b = coeffs[1].neg().map_err(|error| error.src("cas"))?;
    let denominator = cas_mul(vec![Value::Int(2), coeffs[2].clone()])?;
    let first = cas_div(
        cas_add(vec![neg_b.clone(), sqrt_disc.clone()])?,
        denominator.clone(),
    )?;
    let second = cas_div(cas_sub(neg_b, sqrt_disc)?, denominator)?;
    Ok(RootSolution::Finite(vec![
        simplify_cas_value(&first)?,
        simplify_cas_value(&second)?,
    ]))
}

fn solve_numeric_polynomial(coeffs: &[Value], domain: SolveDomain) -> WqResult<RootSolution> {
    let degree = poly_degree(coeffs);
    match degree {
        0 => {
            if numeric_is_zero(&coeffs[0]) {
                return Ok(RootSolution::All);
            }
            Ok(RootSolution::Empty)
        }
        1 => Ok(RootSolution::Finite(vec![eval_exact_numeric_div(
            &coeffs[0].neg().map_err(|e| e.src("cas"))?,
            &coeffs[1],
        )?])),
        2 => {
            let disc = quadratic_discriminant(coeffs)?;
            if coefficients_are_exact_real(coeffs) {
                if numeric_is_negative(&disc) {
                    if domain == SolveDomain::Real {
                        Ok(RootSolution::Empty)
                    } else {
                        solve_exact_complex_quadratic(coeffs, disc)
                    }
                } else {
                    solve_parameterized_polynomial(coeffs, &CasAssumptions::default(), domain, 8)
                }
            } else {
                solve_numeric_quadratic(coeffs, disc, domain)
            }
        }
        _ => {
            if let Some(roots) = solve_exact_monomial_polynomial(coeffs, degree, domain)? {
                return Ok(RootSolution::Finite(roots));
            }
            let roots = solve_monomial_polynomial(coeffs, degree)?;
            if domain == SolveDomain::Real {
                let roots = roots
                    .into_iter()
                    .filter_map(|root| match root {
                        Value::Complex(value)
                            if value.im.abs() <= 1e-12 * value.re.abs().max(1.0) =>
                        {
                            Some(Value::float(value.re))
                        }
                        Value::Complex(_) => None,
                        root => Some(root),
                    })
                    .collect();
                return Ok(RootSolution::Finite(roots));
            }
            Ok(RootSolution::Finite(roots))
        }
    }
}

fn parameterized_poly_degree(coeffs: &[Value], assumptions: &CasAssumptions) -> DegreeDecision {
    for (degree, coefficient) in coeffs.iter().enumerate().rev() {
        match assumptions.prove_zero(coefficient) {
            Truth::Proven => {}
            Truth::Refuted => return DegreeDecision::Degree(degree),
            Truth::Unknown => return DegreeDecision::NeedDecision(coefficient.clone()),
        }
    }
    DegreeDecision::Degree(0)
}

fn solve_parameterized_polynomial(
    coeffs: &[Value],
    assumptions: &CasAssumptions,
    domain: SolveDomain,
    branch_depth: usize,
) -> WqResult<RootSolution> {
    if domain == SolveDomain::Real {
        for coefficient in coeffs {
            if assumptions.prove_real(coefficient) != Truth::Proven {
                return Err(cas_err(format!(
                    "'solve' in the real domain cannot prove coefficient {coefficient} is real; pass '`assuming:real[{coefficient}]' to solve"
                )));
            }
        }
    }
    let degree = match parameterized_poly_degree(coeffs, assumptions) {
        DegreeDecision::Degree(degree) => degree,
        DegreeDecision::NeedDecision(value) => {
            return branch_root_solution(coeffs, assumptions, domain, value, branch_depth);
        }
    };
    let coeff_at = |idx: usize| coeffs.get(idx).cloned().unwrap_or(Value::Int(0));
    match degree {
        0 => match assumptions.prove_zero(&coeffs[0]) {
            Truth::Proven => Ok(RootSolution::All),
            Truth::Refuted => Ok(RootSolution::Empty),
            Truth::Unknown => {
                branch_root_solution(coeffs, assumptions, domain, coeffs[0].clone(), branch_depth)
            }
        },
        1 => {
            let root = cas_div(cas_neg(coeff_at(0))?, coeff_at(1))?;
            Ok(RootSolution::Finite(vec![simplify_cas_value(&root)?]))
        }
        2 => {
            let a = coeff_at(2);
            let b = coeff_at(1);
            let c = coeff_at(0);
            let b_squared = cas_pow(b.clone(), Value::Int(2))?;
            let four_ac = cas_mul(vec![Value::Int(4), a.clone(), c])?;
            let disc = cas_sub(b_squared, four_ac)?;
            solve_parameterized_quadratic(a, b, disc, assumptions, domain)
        }
        _ => Err(cas_err(
            "'solve' with parameters currently supports polynomial degree <= 2",
        )),
    }
}

fn solve_parameterized_quadratic(
    a: Value,
    b: Value,
    disc: Value,
    assumptions: &CasAssumptions,
    domain: SolveDomain,
) -> WqResult<RootSolution> {
    if domain == SolveDomain::Real {
        if assumptions.prove_positive(&disc) == Truth::Proven {
            return Ok(RootSolution::Finite(quadratic_formula_roots(
                a, b, disc, false,
            )?));
        }
        if assumptions.prove_zero(&disc) == Truth::Proven {
            return Ok(RootSolution::Finite(quadratic_formula_roots(
                a, b, disc, true,
            )?));
        }
        if assumptions.prove_negative(&disc) == Truth::Proven {
            return Ok(RootSolution::Empty);
        }
        let candidates = vec![
            ConditionalCase {
                conditions: vec![CasPredicate::Positive(disc.clone())],
                result: RootSolution::Finite(quadratic_formula_roots(
                    a.clone(),
                    b.clone(),
                    disc.clone(),
                    false,
                )?),
            },
            ConditionalCase {
                conditions: vec![CasPredicate::Zero(disc.clone())],
                result: RootSolution::Finite(quadratic_formula_roots(a, b, disc.clone(), true)?),
            },
            ConditionalCase {
                conditions: vec![CasPredicate::Negative(disc)],
                result: RootSolution::Empty,
            },
        ];
        let cases = candidates
            .into_iter()
            .filter(|case| assumptions.prove_predicate(&case.conditions[0]) != Truth::Refuted)
            .collect();
        return Ok(RootSolution::Cases(cases));
    }

    Ok(RootSolution::Finite(quadratic_formula_roots(
        a, b, disc, false,
    )?))
}

fn quadratic_formula_roots(
    a: Value,
    b: Value,
    disc: Value,
    repeated: bool,
) -> WqResult<Vec<Value>> {
    let sqrt_disc = cas_pow(
        disc,
        Value::from_fraction_parts(BigInt::from(1), BigInt::from(2)),
    )?;
    let neg_b = cas_neg(b)?;
    let denom = cas_mul(vec![Value::Int(2), a])?;
    let first = simplify_cas_value(&cas_div(
        cas_add(vec![neg_b.clone(), sqrt_disc.clone()])?,
        denom.clone(),
    )?)?;
    if repeated {
        Ok(vec![first])
    } else {
        Ok(vec![
            first,
            simplify_cas_value(&cas_div(cas_sub(neg_b, sqrt_disc)?, denom)?)?,
        ])
    }
}

fn branch_root_solution(
    coeffs: &[Value],
    assumptions: &CasAssumptions,
    domain: SolveDomain,
    value: Value,
    branch_depth: usize,
) -> WqResult<RootSolution> {
    if branch_depth == 0 {
        return Err(cas_err(
            "'solve' exceeded its conditional branch limit; pass more assumptions",
        ));
    }
    let predicates = [
        CasPredicate::NonZero(value.clone()),
        CasPredicate::Zero(value),
    ];
    let mut cases = Vec::with_capacity(predicates.len());
    for predicate in predicates {
        let branch_assumptions = assumptions.clone().with_predicate(predicate.clone())?;
        let result =
            solve_parameterized_polynomial(coeffs, &branch_assumptions, domain, branch_depth - 1)?;
        push_root_case(&mut cases, predicate, result);
    }
    Ok(RootSolution::Cases(cases))
}

fn push_root_case(
    cases: &mut Vec<ConditionalCase<RootSolution>>,
    predicate: CasPredicate,
    result: RootSolution,
) {
    match result {
        RootSolution::Cases(nested) => {
            for mut case in nested {
                case.conditions.insert(0, predicate.clone());
                cases.push(case);
            }
        }
        result => cases.push(ConditionalCase {
            conditions: vec![predicate],
            result,
        }),
    }
}

fn root_solution_value(solution: RootSolution) -> Value {
    match solution {
        RootSolution::Empty => Value::List(Arc::new(Vec::new())),
        RootSolution::All => Value::Tag(Arc::from("all")),
        RootSolution::Finite(roots) => Value::List(Arc::new(roots)),
        RootSolution::Cases(cases) => named_dict([(
            "cases",
            Value::List(Arc::new(
                cases
                    .into_iter()
                    .map(|case| {
                        named_dict([
                            ("when", condition_value(case.conditions)),
                            ("solutions", root_solution_value(case.result)),
                        ])
                    })
                    .collect(),
            )),
        )]),
    }
}

pub(crate) fn solve_cas(input: &Value, var: &Value) -> WqResult<Value> {
    solve_cas_with_options(input, var, &CasAssumptions::default(), SolveDomain::Complex)
}

#[cfg(test)]
pub(crate) fn solve_cas_with_assumptions(
    input: &Value,
    var: &Value,
    assumptions: &CasAssumptions,
) -> WqResult<Value> {
    solve_cas_with_options(input, var, assumptions, SolveDomain::Complex)
}

pub(crate) fn solve_cas_with_options(
    input: &Value,
    var: &Value,
    assumptions: &CasAssumptions,
    domain: SolveDomain,
) -> WqResult<Value> {
    let var = var_name_from_value(var)?;
    let expr = if let Some((lhs, rhs)) = input.cas_eq_parts() {
        cas_sub(lhs.clone(), rhs.clone())?
    } else {
        simplify_cas_value(input)?
    };
    let solution = match poly_from_expr(&expr, &var) {
        Ok(coeffs) => solve_numeric_polynomial(&coeffs, domain)?,
        Err(_) => {
            let coeffs = poly_from_expr_with_params(&expr, &var)?;
            solve_parameterized_polynomial(&coeffs, assumptions, domain, 8)?
        }
    };
    Ok(root_solution_value(solution))
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
            "'solve_system' expects at least one symbolic variable",
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

#[cfg(test)]
mod exact_monomial_tests {
    use super::*;

    #[test]
    fn exact_cubic_roots_do_not_use_complex64() {
        let x = Value::from_cas_var("x");
        let equation = cas_sub(
            cas_pow(x.clone(), Value::Int(3)).expect("x cubed"),
            Value::Int(2),
        )
        .expect("cubic equation");
        let Value::List(roots) = solve_cas(&equation, &x).expect("cubic roots") else {
            unreachable!("solve returns a list");
        };

        assert_eq!(roots.len(), 3);
        assert_eq!(roots[0].to_string(), "@s 2^(1/3)");
        assert!(
            roots
                .iter()
                .all(|root| !matches!(root, Value::Float(_) | Value::Complex(_)))
        );
    }

    #[test]
    fn exact_quintic_includes_exact_unit_root() {
        let x = Value::from_cas_var("x");
        let equation = cas_sub(
            cas_pow(x.clone(), Value::Int(5)).expect("x fifth"),
            Value::Int(1),
        )
        .expect("quintic equation");
        let Value::List(roots) = solve_cas(&equation, &x).expect("quintic roots") else {
            unreachable!("solve returns a list");
        };

        assert_eq!(roots.first(), Some(&Value::Int(1)));
    }

    #[test]
    fn exact_quadratic_complex_roots_do_not_use_complex64() {
        let x = Value::from_cas_var("x");
        let equation = cas_add(vec![
            cas_pow(x.clone(), Value::Int(2)).expect("x squared"),
            x.clone(),
            Value::Int(1),
        ])
        .expect("quadratic equation");
        let Value::List(roots) = solve_cas(&equation, &x).expect("quadratic roots") else {
            unreachable!("solve returns a list");
        };

        assert_eq!(roots.len(), 2);
        assert!(
            roots
                .iter()
                .all(|root| !matches!(root, Value::Float(_) | Value::Complex(_)))
        );
    }

    #[test]
    fn exact_algebraic_monomial_coefficients_do_not_use_complex64() {
        let x = Value::from_cas_var("x");
        let sqrt_two = cas_pow(
            Value::Int(2),
            Value::from_fraction_parts(BigInt::from(1), BigInt::from(2)),
        )
        .expect("exact square root");
        let equation = cas_sub(
            cas_pow(x.clone(), Value::Int(3)).expect("x cubed"),
            sqrt_two,
        )
        .expect("algebraic monomial equation");
        let Value::List(roots) = solve_cas(&equation, &x).expect("algebraic monomial roots") else {
            unreachable!("solve returns a list");
        };

        assert_eq!(roots.len(), 3);
        assert!(
            roots
                .iter()
                .all(|root| !matches!(root, Value::Float(_) | Value::Complex(_)))
        );
    }
}

#[cfg(test)]
mod diagnostic_wording_tests {
    use super::*;

    #[test]
    fn solve_messages_quote_callable_identifiers() {
        let err = solve_monomial_polynomial(&[Value::Int(1), Value::Int(1), Value::Int(1)], 2)
            .expect_err("non-monomial polynomial should fail");
        assert_eq!(
            err.msg.as_deref(),
            Some(
                "'solve' currently supports degree 2 only for equations of the form a*x^2 + b = 0"
            )
        );

        let err =
            normalize_system_equations(&Value::Int(1)).expect_err("non-list system should fail");
        assert_eq!(
            err.msg.as_deref(),
            Some("'solve_system' expects a list of equations or expressions")
        );

        let vars = Value::List(Arc::new(vec![
            Value::from_cas_var("x"),
            Value::from_cas_var("x"),
        ]));
        let err = parse_system_var_names(&vars).expect_err("duplicate variable should fail");
        assert_eq!(
            err.msg.as_deref(),
            Some("'solve_system' variable 'x' appears more than once")
        );
    }
}
