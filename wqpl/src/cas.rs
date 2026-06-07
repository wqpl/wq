#[macro_use]
mod debug;
mod eqsat;
mod expand_factor;
mod format;
mod numeric;
mod poly;
mod rewrite;
mod simplify;
mod solve;
#[cfg(test)]
mod tests;
mod value_ext;

pub(crate) use debug::{cas_debug_enabled, cas_debug_log_depth};
use expand_factor::{
    eval_numeric_binary_gcd, expand_expr, extract_algebraic_content, factor_expr, split_off_results,
};
pub(crate) use expand_factor::{expand_cas, factor_cas};
use format::{format_expr, sort_canonical};
pub(crate) use numeric::{
    cas_err, eval_exact_numeric_div, eval_numeric_binary, eval_numeric_cas, numeric_add,
    numeric_div, numeric_is_negative, numeric_is_one, numeric_is_zero, numeric_mul, numeric_pow,
    numeric_sub,
};
use numeric::{ensure_expr_arg, eval_numeric_call, try_eval_with_const_resolve};
use poly::{collect_single_poly_var, try_exact_polynomial_division};
pub(crate) use poly::{
    extract_linear_coefficients, poly_add, poly_degree, poly_derivative, poly_divide,
    poly_evaluate, poly_from_expr, poly_gcd, poly_interpolate, poly_is_zero, poly_mul, poly_neg,
    poly_resultant, poly_scalar_mul, poly_sub, poly_to_expr, poly_trim, square_free_factor,
};
#[cfg(test)]
use rewrite::rewrite_expr;
use rewrite::try_cancel_affine_over_factor;
pub(crate) use rewrite::{
    cas_product, contains_cas_var, infer_single_cas_var, normalize_root_objective_cas, rewrite_cas,
    rewrite_loop,
};
use simplify::{
    cas_add, cas_mul, cas_neg, cas_sub, common_numeric_gcd, rebuild_scaled_term, split_add_term,
    split_mul_factor, substitute_expr, var_name_from_value,
};
pub(crate) use simplify::{
    cas_binary_expr, cas_call_expr, cas_div, cas_pow, cas_unary_expr, extract_perfect_power_factor,
    simplify_cas_value, substitute_cas,
};
pub(crate) use solve::{solve_cas, solve_system_cas};

pub(crate) mod diff;
pub(crate) mod integrate;
pub(crate) mod limit;
