#[macro_use]
mod debug;
mod assumption;
mod expand_factor;
mod format;
mod numeric;
mod poly;
mod quote;
mod rewrite;
mod root;
mod simplify;
mod solve;
#[cfg(test)]
mod tests;
mod value_ext;

pub(crate) use assumption::CasAssumptions;
pub(crate) use debug::CasDebug;
#[cfg(test)]
use expand_factor::expand_cas;
use expand_factor::{
    eval_numeric_binary_gcd, expand_expr, extract_algebraic_content, factor_expr, split_off_results,
};
pub(crate) use expand_factor::{expand_cas_with_debug, factor_cas};
use format::{format_cas_equation, format_cas_value, sort_canonical};
pub(crate) use numeric::{
    cas_err, ensure_expr_arg, eval_exact_numeric_div, eval_numeric_binary, eval_numeric_cas,
    numeric_add, numeric_div, numeric_is_negative, numeric_is_one, numeric_is_zero, numeric_mul,
    numeric_pow, numeric_sub,
};
use numeric::{eval_numeric_call, try_eval_with_const_resolve};
use poly::{collect_single_poly_var, try_exact_polynomial_division};
pub(crate) use poly::{
    extract_linear_coefficients, extract_linear_coefficients_with_params, poly_add, poly_const_mul,
    poly_degree, poly_derivative, poly_divide, poly_evaluate, poly_from_expr,
    poly_from_expr_with_params, poly_gcd, poly_interpolate, poly_is_zero, poly_mul, poly_neg,
    poly_resultant, poly_sub, poly_to_expr, poly_trim, square_free_factor,
};
pub(crate) use quote::{cas_special_call_name, cas_symbolic_call_expr};
#[cfg(test)]
use rewrite::rewrite_cas;
#[cfg(test)]
use rewrite::rewrite_expr;
use rewrite::try_cancel_affine_over_factor;
pub(crate) use rewrite::{
    cas_product, contains_cas_var, infer_single_cas_var, normalize_root_objective_cas,
    rewrite_cas_with_debug, rewrite_loop_with_debug,
};
pub(crate) use root::resolve_cas_root;
pub(crate) use simplify::{
    cas_add, cas_binary_expr, cas_call_expr, cas_div, cas_mul, cas_neg, cas_pow, cas_sub,
    cas_unary_expr, extract_perfect_power_factor, simplify_cas_value,
    simplify_cas_value_with_debug, substitute_cas, substitute_cas_bindings, with_cas_div_cache,
};
use simplify::{
    common_numeric_gcd, rebuild_scaled_term, split_add_term, split_mul_factor, substitute_expr,
    var_name_from_value,
};
pub(crate) use solve::{
    SolveDomain, solve_cas, solve_cas_with_options, solve_system_cas_with_assumptions,
    solve_system_infer_cas_with_assumptions,
};
#[cfg(test)]
pub(crate) use solve::{solve_cas_with_assumptions, solve_system_cas, solve_system_infer_cas};

pub(crate) mod diff;
pub(crate) mod integrate;
pub(crate) mod limit;
