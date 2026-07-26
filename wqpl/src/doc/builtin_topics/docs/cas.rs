use super::super::super::model::{BuiltinDoc, DocExample, ExampleExpectation};
use crate::builtins::BuiltinEnum;

const EQ_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Build a symbolic equation",
    code: "eq[@s x^2;1]",
    expectation: ExampleExpectation::ResultContains("x^2 = 1"),
}];

const SIMPLIFY_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Combine like terms",
    code: "simplify @s 2*x+x+1",
    expectation: ExampleExpectation::ResultContains("3*x + 1"),
}];

const REWRITE_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Cancel a common factor",
    code: "rewrite @s (x+1)/(x+1)",
    expectation: ExampleExpectation::ResultContains("1"),
}];

const NUMERIC_EXAMPLES: &[DocExample] = &[
    DocExample {
        title: "Evaluate symbolic constants and functions",
        code: "numeric @s sin[0]+cos[0]",
        expectation: ExampleExpectation::ResultContains("1.0"),
    },
    DocExample {
        title: "Evaluate after binding symbols",
        code: "@s sin[x]+y | numeric[`x:0;`y:2]",
        expectation: ExampleExpectation::ResultContains("2.0"),
    },
];

const DIFF_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Differentiate a polynomial",
    code: "diff @s x^3",
    expectation: ExampleExpectation::ResultContains("3*x^2"),
}];

const SUBSTITUTE_EXAMPLES: &[DocExample] = &[
    DocExample {
        title: "Bind a symbol with a named argument",
        code: "@s x^2+y | substitute[`x:2]",
        expectation: ExampleExpectation::ResultContains("y + 4"),
    },
    DocExample {
        title: "Apply a list of equations",
        code: "substitute[@s x+y;(eq[@s x;2];eq[@s y;3])]",
        expectation: ExampleExpectation::ResultContains("5"),
    },
];

const EXPAND_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Expand a binomial",
    code: "expand @s (x+1)^2",
    expectation: ExampleExpectation::ResultContains("x^2 + 2*x + 1"),
}];

const FACTOR_COMMON_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Pull out a common factor",
    code: "factor_common @s x^2+x",
    expectation: ExampleExpectation::ResultContains("x*(x + 1)"),
}];

const FACTOR_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Factor a polynomial",
    code: "factor @s x^2-1",
    expectation: ExampleExpectation::ResultContains("(x - 1)*(x + 1)"),
}];

const INTEGRATE_EXAMPLES: &[DocExample] = &[
    DocExample {
        title: "Compute a definite integral",
        code: "integrate[@s x;@s x;0;1]",
        expectation: ExampleExpectation::ResultContains("1/2"),
    },
    DocExample {
        title: "Treat other symbols as parameters",
        code: "integrate[@s sin[a*x];@s x]",
        expectation: ExampleExpectation::ResultContains("-cos[a*x]/a"),
    },
    DocExample {
        title: "Integrate an affine denominator",
        code: "integrate[@s 1/(x+a);@s x]",
        expectation: ExampleExpectation::ResultContains("ln[abs[x + a]]"),
    },
    DocExample {
        title: "Recover sine integral form",
        code: "integrate[@s sin[x]/x;@s x]",
        expectation: ExampleExpectation::ResultContains("si[x]"),
    },
    DocExample {
        title: "Recover cosine integral form",
        code: "integrate[@s cos[x]/x;@s x]",
        expectation: ExampleExpectation::ResultContains("ci[x]"),
    },
];

const LIMIT_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Use a classic trigonometric limit",
    code: "limit[@s sin[x]/x;0]",
    expectation: ExampleExpectation::ResultContains("1"),
}];

const SOLVE_EXAMPLES: &[DocExample] = &[
    DocExample {
        title: "Solve a single-variable equation",
        code: "solve @s x-2",
        expectation: ExampleExpectation::ResultContains("2"),
    },
    DocExample {
        title: "Solve with symbolic parameters",
        code: "solve[@s a*x;@s x]",
        expectation: ExampleExpectation::ResultContains("`cases"),
    },
    DocExample {
        title: "Keep exact real quadratic roots when possible",
        code: "solve[@s x^2-2]",
        expectation: ExampleExpectation::ResultContains("2^(1/2)"),
    },
    DocExample {
        title: "Keep exact binomial quintic roots",
        code: "solve[@s x^5-1]",
        expectation: ExampleExpectation::ResultContains("sin[2/5*pi]"),
    },
    DocExample {
        title: "Restrict roots to the real domain",
        code: "solve[@s x^2+1;`domain:`real]",
        expectation: ExampleExpectation::ResultContains("()"),
    },
];

const SOLVE_SYSTEM_EXAMPLES: &[DocExample] = &[
    DocExample {
        title: "Solve a linear system",
        code: "solve_system[(eq[@s x;2];eq[@s y;3])]",
        expectation: ExampleExpectation::ResultContains("`x:2"),
    },
    DocExample {
        title: "Solve a system with parameters",
        code: "solve_system[(eq[@s 2*x+y;@s b];eq[@s x-y;@s c]);(@s x;@s y)]",
        expectation: ExampleExpectation::ResultContains("`x:(c + b)/3"),
    },
];

const BRENT_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Find a bracketed real root",
    code: "brent[@s x^2-2;1;2]",
    expectation: ExampleExpectation::ResultContains("1.414"),
}];

const NEWTON_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Find a real root from an initial guess",
    code: "newton[@s x^2-2;1]",
    expectation: ExampleExpectation::ResultContains("1.414"),
}];

pub(super) const EQ: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Eq,
    summary: "Build a symbolic equation.",
    details: "`eq[lhs;rhs]` wraps two values as a CAS equation without solving it. Use `@s` for symbolic sides when names should remain variables; equation values can be passed to `solve`, `substitute`, `brent`, and `newton`.",
    examples: EQ_EXAMPLES,
    related: &["solve", "substitute", "brent", "newton"],
};

pub(super) const SIMPLIFY: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Simplify,
    summary: "Simplify a symbolic expression.",
    details: "`simplify[expr]` runs the CAS simplifier over `expr`, combining numeric parts, like terms, and algebraic identities while preserving unresolved symbolic variables.",
    examples: SIMPLIFY_EXAMPLES,
    related: &["rewrite", "expand", "factor_common"],
};

pub(super) const REWRITE: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Rewrite,
    summary: "Normalize a symbolic expression with rewrite rules.",
    details: "`rewrite[expr]` applies targeted CAS rewrites such as cancellations and inverse identities, then returns the rewritten expression. It is useful before simplification, calculus, or numeric evaluation.",
    examples: REWRITE_EXAMPLES,
    related: &["simplify", "factor_common", "diff"],
};

pub(super) const NUMERIC: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Numeric,
    summary: "Evaluate a symbolic expression numerically.",
    details: "`numeric[expr]` evaluates CAS constants, functions, and resolved limit forms once all symbolic variables have been removed or substituted. Named arguments bind symbols before evaluation, so ``expr|numeric[`x:1]`` substitutes `x` and then evaluates. It errors when the expression still depends on a variable.",
    examples: NUMERIC_EXAMPLES,
    related: &["substitute", "simplify", "float"],
};

pub(super) const DIFF: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Diff,
    summary: "Differentiate a symbolic expression.",
    details: "`diff[expr]` infers the variable when `expr` contains exactly one symbolic variable. `diff[expr;var]` differentiates with respect to an explicit symbolic variable. `D` is an alias.",
    examples: DIFF_EXAMPLES,
    related: &["integrate", "simplify"],
};

pub(super) const SUBSTITUTE: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Substitute,
    summary: "Replace symbols or subexpressions in a CAS expression.",
    details: "``substitute[expr;`name:val...]`` binds symbols by name. `substitute[expr;var;val]` replaces `var` with `val`. The two-argument form accepts one `eq[lhs;rhs]` or a list of equations, applying list entries in order.",
    examples: SUBSTITUTE_EXAMPLES,
    related: &["eq", "numeric", "simplify"],
};

pub(super) const EXPAND: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Expand,
    summary: "Expand products and powers in a symbolic expression.",
    details: "`expand[expr]` distributes symbolic products and supported integer powers, producing a sum-of-terms form that is useful before collecting, solving, or comparing expressions.",
    examples: EXPAND_EXAMPLES,
    related: &["factor", "factor_common", "simplify"],
};

pub(super) const FACTOR_COMMON: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::FactorCommon,
    summary: "Extract common factors from a symbolic sum.",
    details: "`factor_common[expr]` looks for shared numeric and symbolic factors across additive terms. It is a lightweight common-factor pass, distinct from full polynomial factorization.",
    examples: FACTOR_COMMON_EXAMPLES,
    related: &["factor", "expand", "simplify"],
};

pub(super) const FACTOR: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Factor,
    summary: "Factor a polynomial expression.",
    details: "`factor[expr]` infers the polynomial variable when possible. `factor[expr;var]` uses an explicit symbolic variable, while `factor[expr;T]` and `factor[expr;T;var]` enable complex quadratic factors.",
    examples: FACTOR_EXAMPLES,
    related: &["factor_common", "expand", "solve"],
};

pub(super) const INTEGRATE: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Integrate,
    summary: "Integrate a symbolic expression.",
    details: "`integrate[expr]` infers one symbolic variable, `integrate[expr;var]` uses an explicit variable, and `integrate[expr;var;lower;upper]` computes a definite integral. Supported affine table rules treat other symbols as parameters when an explicit variable is passed. Inside `@s`, `integrate[...]` creates an unevaluated binding-aware integral instead of calling this evaluator. `I` is an alias.",
    examples: INTEGRATE_EXAMPLES,
    related: &["diff", "limit"],
};

pub(super) const LIMIT: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Limit,
    summary: "Compute a symbolic limit.",
    details: "`limit[expr;point]` infers the only symbolic variable. `limit[expr;var;point]` uses an explicit variable. Points accept finite values, runtime infinities such as `inf` and `-inf`, or symbolic infinity such as `@s inf` and `@s -inf` (`@s oo` remains an alias for positive infinity). Additional `var;point` pairs are applied in sequence. Use named argument `direction` with `@s+` or `@s-` to request a one-sided limit for the last pair. Inside `@s`, the same forms create an unevaluated binding-aware limit. Quoted and evaluator forms share the same argument validation.",
    examples: LIMIT_EXAMPLES,
    related: &["diff", "integrate", "numeric"],
};

pub(super) const SOLVE: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Solve,
    summary: "Solve a single-variable symbolic equation.",
    details: "`solve[expr]` treats `expr` as equal to zero and infers the only symbolic variable.
`solve[expr;var]` uses an explicit variable, and equation inputs solve `lhs = rhs`.
It handles linear and quadratic polynomials, including coefficients with other symbolic parameters when the variable is explicit.
When a symbolic coefficient can change the degree, the result contains `cases` with `when` conditions and branch `solutions`.
A finite solution is a list, the `all` tag is the identity result, and an empty list means no solution.
Named argument `assuming` narrows the cases. Named argument `domain` accepts the `complex` tag, which is the default, or the `real` tag.
Parameterized real-domain solves require `real` assumptions for symbolic coefficients.
Exact quadratic coefficients keep exact real or complex roots.
For degree greater than 2, `solve` currently supports binomials of the form `a*x^n + b = 0`; exact coefficients produce exact symbolic roots and approximate coefficients produce approximate roots.
General higher-degree polynomials such as `x^3+x-1` are not solved symbolically; use `brent` or `newton` for numeric real roots.",
    examples: SOLVE_EXAMPLES,
    related: &["eq", "solve_system", "brent", "newton"],
};

pub(super) const SOLVE_SYSTEM: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::SolveSystem,
    summary: "Solve a linear symbolic system.",
    details: "`solve_system[eqs]` infers symbolic variables and returns a dict keyed by variable name for a unique solution.
`solve_system[eqs;vars]` uses an explicit variable list, which also controls dict order.
Symbols outside that explicit list are treated as parameters.
When a symbolic determinant or pivot can be zero, the result contains `cases` with `when` conditions and branch `solution` values.
Named argument `assuming` narrows those cases.
A dependent or underdetermined system returns `solution` bindings plus a `parameters` list of fresh symbols.
The `none` tag means the system is inconsistent.
The symbolic square-system path supports up to 12 variables, and Gaussian elimination handles other linear shapes.",
    examples: SOLVE_SYSTEM_EXAMPLES,
    related: &["solve", "eq", "substitute"],
};

pub(super) const BRENT: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Brent,
    summary: "Find a real root in a bracketed interval.",
    details: "`brent[expr;a;b]` treats `expr` as equal to zero, infers its single variable, and finds a real root between finite bounds `a` and `b`.
The interval must bracket a sign change; optional tolerance and iteration-limit arguments follow the bounds.",
    examples: BRENT_EXAMPLES,
    related: &["newton", "solve", "eq", "numeric"],
};

pub(super) const NEWTON: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Newton,
    summary: "Find a real root with Newton iteration.",
    details: "`newton[expr;x0]` treats `expr` as equal to zero, infers its single variable, differentiates it symbolically, and iterates from the real initial guess `x0`.
Optional tolerance and iteration-limit arguments follow the guess.",
    examples: NEWTON_EXAMPLES,
    related: &["brent", "diff", "solve", "numeric"],
};
