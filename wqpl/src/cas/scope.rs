use std::collections::BTreeSet;

use crate::value::Value;
use crate::value::cas::{CasScope, CasSymbol};

/// Close every free occurrence of `name` over a new local CAS scope.
pub(crate) fn close_cas_scope(body: &Value, name: &str) -> CasScope {
    CasScope::new(close_value(body, name, 0), CasSymbol::new(name))
}

fn close_value(value: &Value, name: &str, target_index: u32) -> Value {
    if value.cas_var_name() == Some(name) {
        return Value::from_cas_bound_var(target_index);
    }
    if value.cas_var_name().is_some()
        || value.cas_bound_var().is_some()
        || value.cas_const().is_some()
        || !value.is_cas()
    {
        return value.clone();
    }
    if let Some((op, args)) = value.cas_op_parts() {
        return Value::from_cas_op(
            op,
            args.iter()
                .map(|arg| close_value(arg, name, target_index))
                .collect(),
        );
    }
    if let Some((function, args)) = value.cas_function_parts() {
        return Value::from_cas_function(
            function,
            args.iter()
                .map(|arg| close_value(arg, name, target_index))
                .collect(),
        );
    }
    if let Some((head, args)) = value.cas_apply_parts() {
        return Value::from_cas_apply(
            head.as_str(),
            args.iter()
                .map(|arg| close_value(arg, name, target_index))
                .collect(),
        );
    }
    if let Some((arg_name, arg)) = value.cas_named_arg_parts() {
        return Value::from_cas_named_arg(arg_name.as_str(), close_value(arg, name, target_index));
    }
    if let Some((scope, bounds)) = value.cas_integral_parts() {
        let nested = CasScope::new(
            close_value(scope.body(), name, target_index + 1),
            scope.hint().clone(),
        );
        let bounds = bounds.map(|(lower, upper)| {
            (
                close_value(lower, name, target_index),
                close_value(upper, name, target_index),
            )
        });
        return Value::from_cas_integral(nested, bounds);
    }
    if let Some((scope, point, direction)) = value.cas_limit_parts() {
        let nested = CasScope::new(
            close_value(scope.body(), name, target_index + 1),
            scope.hint().clone(),
        );
        return Value::from_cas_limit(nested, close_value(point, name, target_index), direction);
    }
    if let Some((lhs, rhs)) = value.cas_eq_parts() {
        return Value::from_cas_eq(
            close_value(lhs, name, target_index),
            close_value(rhs, name, target_index),
        );
    }
    if let Some(predicate) = value.cas_predicate() {
        return Value::from_cas_predicate(predicate.with_expr(close_value(
            predicate.expr(),
            name,
            target_index,
        )));
    }
    value.clone()
}

/// Open a scope using a collision-free free variable.
pub(crate) fn open_cas_scope(scope: &CasScope) -> (Value, Value) {
    let mut used = BTreeSet::new();
    collect_free_cas_vars(scope.body(), &mut used);
    let name = fresh_name(scope.hint().as_str(), &used);
    let var = Value::from_cas_var(&name);
    (open_cas_scope_with_value(scope, &var), var)
}

/// Open a scope with a caller-selected free expression.
pub(crate) fn open_cas_scope_with_value(scope: &CasScope, replacement: &Value) -> Value {
    open_value(scope.body(), replacement, 0)
}

fn open_value(value: &Value, replacement: &Value, target_index: u32) -> Value {
    if let Some(index) = value.cas_bound_var() {
        if index == target_index {
            return replacement.clone();
        }
        return Value::from_cas_bound_var(if index > target_index {
            index - 1
        } else {
            index
        });
    }
    if value.cas_var_name().is_some() || value.cas_const().is_some() || !value.is_cas() {
        return value.clone();
    }
    if let Some((op, args)) = value.cas_op_parts() {
        return Value::from_cas_op(
            op,
            args.iter()
                .map(|arg| open_value(arg, replacement, target_index))
                .collect(),
        );
    }
    if let Some((function, args)) = value.cas_function_parts() {
        return Value::from_cas_function(
            function,
            args.iter()
                .map(|arg| open_value(arg, replacement, target_index))
                .collect(),
        );
    }
    if let Some((head, args)) = value.cas_apply_parts() {
        return Value::from_cas_apply(
            head.as_str(),
            args.iter()
                .map(|arg| open_value(arg, replacement, target_index))
                .collect(),
        );
    }
    if let Some((arg_name, arg)) = value.cas_named_arg_parts() {
        return Value::from_cas_named_arg(
            arg_name.as_str(),
            open_value(arg, replacement, target_index),
        );
    }
    if let Some((scope, bounds)) = value.cas_integral_parts() {
        let nested = CasScope::new(
            open_value(scope.body(), replacement, target_index + 1),
            scope.hint().clone(),
        );
        let bounds = bounds.map(|(lower, upper)| {
            (
                open_value(lower, replacement, target_index),
                open_value(upper, replacement, target_index),
            )
        });
        return Value::from_cas_integral(nested, bounds);
    }
    if let Some((scope, point, direction)) = value.cas_limit_parts() {
        let nested = CasScope::new(
            open_value(scope.body(), replacement, target_index + 1),
            scope.hint().clone(),
        );
        return Value::from_cas_limit(
            nested,
            open_value(point, replacement, target_index),
            direction,
        );
    }
    if let Some((lhs, rhs)) = value.cas_eq_parts() {
        return Value::from_cas_eq(
            open_value(lhs, replacement, target_index),
            open_value(rhs, replacement, target_index),
        );
    }
    if let Some(predicate) = value.cas_predicate() {
        return Value::from_cas_predicate(predicate.with_expr(open_value(
            predicate.expr(),
            replacement,
            target_index,
        )));
    }
    value.clone()
}

pub(crate) fn collect_free_cas_vars(value: &Value, names: &mut BTreeSet<String>) {
    if let Some(name) = value.cas_var_name() {
        names.insert(name.to_string());
        return;
    }
    if value.cas_bound_var().is_some() || value.cas_const().is_some() || !value.is_cas() {
        return;
    }
    if let Some((_, args)) = value.cas_op_parts() {
        for arg in args {
            collect_free_cas_vars(arg, names);
        }
        return;
    }
    if let Some((_, args)) = value.cas_function_parts() {
        for arg in args {
            collect_free_cas_vars(arg, names);
        }
        return;
    }
    if let Some((_, args)) = value.cas_apply_parts() {
        for arg in args {
            collect_free_cas_vars(arg, names);
        }
        return;
    }
    if let Some((_, arg)) = value.cas_named_arg_parts() {
        collect_free_cas_vars(arg, names);
        return;
    }
    if let Some((scope, bounds)) = value.cas_integral_parts() {
        collect_free_cas_vars(scope.body(), names);
        if let Some((lower, upper)) = bounds {
            collect_free_cas_vars(lower, names);
            collect_free_cas_vars(upper, names);
        }
        return;
    }
    if let Some((scope, point, _)) = value.cas_limit_parts() {
        collect_free_cas_vars(scope.body(), names);
        collect_free_cas_vars(point, names);
        return;
    }
    if let Some((lhs, rhs)) = value.cas_eq_parts() {
        collect_free_cas_vars(lhs, names);
        collect_free_cas_vars(rhs, names);
        return;
    }
    if let Some(predicate) = value.cas_predicate() {
        collect_free_cas_vars(predicate.expr(), names);
    }
}

pub(crate) fn fresh_name(hint: &str, used: &BTreeSet<String>) -> String {
    if !used.contains(hint) {
        return hint.to_string();
    }
    for suffix in 1usize.. {
        let candidate = format!("{hint}{suffix}");
        if !used.contains(&candidate) {
            return candidate;
        }
    }
    unreachable!("an unbounded identifier suffix sequence contains a fresh name")
}

#[cfg(test)]
mod tests {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    use super::*;
    use crate::value::cas::CasOp;

    #[test]
    fn closing_and_opening_preserves_free_name_collisions() {
        let body = Value::from_cas_op(
            CasOp::Add,
            vec![Value::from_cas_var("x"), Value::from_cas_var("y")],
        );
        let scope = close_cas_scope(&body, "x");
        let replaced = crate::cas::substitute_cas(
            scope.body(),
            &Value::from_cas_var("y"),
            &Value::from_cas_var("x"),
        )
        .expect("free substitution");
        let scope = CasScope::new(replaced, scope.hint().clone());
        let (opened, var) = open_cas_scope(&scope);

        assert_eq!(var.cas_var_name(), Some("x1"));
        assert_eq!(opened.to_string(), "@s x + x1");
    }

    #[test]
    fn alpha_equivalent_scopes_compare_equal() {
        let x = close_cas_scope(&Value::from_cas_var("x"), "x");
        let t = close_cas_scope(&Value::from_cas_var("t"), "t");
        let x_integral = Value::from_cas_integral(x, None);
        let t_integral = Value::from_cas_integral(t, None);
        let mut x_hash = DefaultHasher::new();
        let mut t_hash = DefaultHasher::new();
        x_integral.hash(&mut x_hash);
        t_integral.hash(&mut t_hash);

        assert_eq!(x_integral, t_integral);
        assert_eq!(x_hash.finish(), t_hash.finish());
    }
}
