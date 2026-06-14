use crate::builtins::BuiltinEnum;

use super::super::super::model::{BuiltinDoc, DocExample, ExampleExpectation};

const NEG_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Negate every leaf",
    code: "neg (-2;3)",
    expectation: ExampleExpectation::ResultContains("(2;-3)"),
}];

const ABS_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Take absolute values",
    code: "abs (-3;4;-5)",
    expectation: ExampleExpectation::ResultContains("(3;4;5)"),
}];

const SGN_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Classify signs",
    code: "sgn (-2;0;3)",
    expectation: ExampleExpectation::ResultContains("(-1;0;1)"),
}];

const SQRT_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Take square roots",
    code: "sqrt (4;9;16)",
    expectation: ExampleExpectation::ResultContains("(2.0;3.0;4.0)"),
}];

const EXP_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Evaluate e to a power",
    code: "exp 0",
    expectation: ExampleExpectation::ResultContains("1.0"),
}];

const LN_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Evaluate the natural log",
    code: "ln 1",
    expectation: ExampleExpectation::ResultContains("0.0"),
}];

const LOG2_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Evaluate a base-2 log",
    code: "log2 8",
    expectation: ExampleExpectation::ResultContains("3.0"),
}];

const LOG10_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Evaluate a base-10 log",
    code: "log10 1000",
    expectation: ExampleExpectation::ResultContains("3.0"),
}];

const FLOOR_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Floor to decimal places",
    code: "floor[3.14159;2]",
    expectation: ExampleExpectation::ResultContains("3.14"),
}];

const CEIL_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Ceil to decimal places",
    code: "ceil[3.14159;2]",
    expectation: ExampleExpectation::ResultContains("3.15"),
}];

const ROUND_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Round to decimal places",
    code: "round[3.14159;2]",
    expectation: ExampleExpectation::ResultContains("3.14"),
}];

const SIN_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Evaluate sine",
    code: "sin 0",
    expectation: ExampleExpectation::ResultContains("0.0"),
}];

const COS_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Evaluate cosine",
    code: "cos 0",
    expectation: ExampleExpectation::ResultContains("1.0"),
}];

const TAN_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Evaluate tangent",
    code: "tan 0",
    expectation: ExampleExpectation::ResultContains("0.0"),
}];

const SEC_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Evaluate secant",
    code: "sec 0",
    expectation: ExampleExpectation::ResultContains("1.0"),
}];

const CSC_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Evaluate cosecant",
    code: "csc 1.5707963267948966",
    expectation: ExampleExpectation::ResultContains("1.0"),
}];

const COT_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Evaluate cotangent",
    code: "cot 0.7853981633974483",
    expectation: ExampleExpectation::ResultContains("1.0"),
}];

const ARCSIN_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Evaluate inverse sine",
    code: "arcsin 1",
    expectation: ExampleExpectation::ResultContains("1.5707963267948966"),
}];

const ARCCOS_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Evaluate inverse cosine",
    code: "arccos 1",
    expectation: ExampleExpectation::ResultContains("0.0"),
}];

const ARCTAN_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Evaluate inverse tangent",
    code: "arctan 1",
    expectation: ExampleExpectation::ResultContains("0.7853981633974483"),
}];

const SINH_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Evaluate hyperbolic sine",
    code: "sinh 0",
    expectation: ExampleExpectation::ResultContains("0.0"),
}];

const COSH_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Evaluate hyperbolic cosine",
    code: "cosh 0",
    expectation: ExampleExpectation::ResultContains("1.0"),
}];

const TANH_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Evaluate hyperbolic tangent",
    code: "tanh 0",
    expectation: ExampleExpectation::ResultContains("0.0"),
}];

const ARCSINH_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Evaluate inverse hyperbolic sine",
    code: "arcsinh 0",
    expectation: ExampleExpectation::ResultContains("0.0"),
}];

const ARCCOSH_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Evaluate inverse hyperbolic cosine",
    code: "arccosh 1",
    expectation: ExampleExpectation::ResultContains("0.0"),
}];

const ARCTANH_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Evaluate inverse hyperbolic tangent",
    code: "arctanh 0",
    expectation: ExampleExpectation::ResultContains("0.0"),
}];

const LOG_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Evaluate a log in a chosen base",
    code: "log[8;2]",
    expectation: ExampleExpectation::ResultContains("3.0"),
}];

const ARCTAN2_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Evaluate quadrant-aware inverse tangent",
    code: "arctan2[1;1]",
    expectation: ExampleExpectation::ResultContains("0.7853981633974483"),
}];

const ERF_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Evaluate the error function",
    code: "erf 0",
    expectation: ExampleExpectation::ResultContains("0.0"),
}];

const ERFC_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Evaluate the complementary error function",
    code: "erfc 0",
    expectation: ExampleExpectation::ResultContains("1.0"),
}];

const GAMMA_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Evaluate the gamma function",
    code: "gamma 5",
    expectation: ExampleExpectation::ResultContains("24.0"),
}];

const LNGAMMA_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Evaluate log gamma",
    code: "lngamma 5",
    expectation: ExampleExpectation::ResultContains("3.1780538303479458"),
}];

const SI_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Evaluate the sine integral",
    code: "si 0",
    expectation: ExampleExpectation::ResultContains("0.0"),
}];

const CI_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Evaluate the cosine integral",
    code: "ci 1",
    expectation: ExampleExpectation::ResultContains("0.33740392290096816"),
}];

const EI_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Evaluate the exponential integral",
    code: "ei 1",
    expectation: ExampleExpectation::ResultContains("1.895117816355937"),
}];

const EN_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Evaluate a generalized exponential integral",
    code: "en[1;1]",
    expectation: ExampleExpectation::ResultContains("0.2193839343955205"),
}];

const ELLPK_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Evaluate a complete elliptic integral",
    code: "ellpk 1",
    expectation: ExampleExpectation::ResultContains("1.5707963267948966"),
}];

const ELLPE_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Evaluate a complete elliptic integral",
    code: "ellpe 1",
    expectation: ExampleExpectation::ResultContains("1.5707963267948966"),
}];

const ELLIK_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Evaluate an incomplete elliptic integral",
    code: "ellik[0;0.5]",
    expectation: ExampleExpectation::ResultContains("0.0"),
}];

const ELLIE_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Evaluate an incomplete elliptic integral",
    code: "ellie[0;0.5]",
    expectation: ExampleExpectation::ResultContains("0.0"),
}];

const HEAVISIDE_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Evaluate a step function",
    code: "heaviside (-1;0;2)",
    expectation: ExampleExpectation::ResultContains("(0.0;0.5;1.0)"),
}];

const DELTA_EXAMPLES: &[DocExample] = &[DocExample {
    title: "Evaluate away from the singularity",
    code: "delta 2",
    expectation: ExampleExpectation::ResultContains("0.0"),
}];

pub(super) const NEG: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Neg,
    summary: "Negate numeric values.",
    details: "`neg[xs]` broadcasts through nested values, returning the additive inverse of each numeric leaf. It also works on complex, fraction, algebraic, and CAS values.",
    examples: NEG_EXAMPLES,
    related: &["abs", "sgn", "-"],
};

pub(super) const ABS: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Abs,
    summary: "Return absolute values or complex magnitudes.",
    details: "`abs[xs]` broadcasts through nested values. Real-like values keep their exact numeric form where possible; complex values return their magnitude.",
    examples: ABS_EXAMPLES,
    related: &["neg", "sgn"],
};

pub(super) const SGN: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Sgn,
    summary: "Return the sign of real numeric values.",
    details: "`sgn[xs]` broadcasts through nested real-like values, returning `-1`, `0`, or `1` for negative, zero, and positive leaves.",
    examples: SGN_EXAMPLES,
    related: &["abs", "heaviside"],
};

pub(super) const SQRT: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Sqrt,
    summary: "Return square roots.",
    details: "`sqrt[xs]` broadcasts through nested numeric leaves. Real inputs that leave the real domain can produce complex results, and CAS expressions remain symbolic.",
    examples: SQRT_EXAMPLES,
    related: &["^", "ln", "exp"],
};

pub(super) const EXP: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Exp,
    summary: "Return e raised to each value.",
    details: "`exp[xs]` broadcasts through nested numeric leaves and evaluates the natural exponential. Complex and CAS inputs are supported by the same symbolic/numeric path as other elementary functions.",
    examples: EXP_EXAMPLES,
    related: &["ln", "log", "^"],
};

pub(super) const LN: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Ln,
    summary: "Return natural logarithms.",
    details: "`ln[xs]` broadcasts through nested numeric leaves. Negative real inputs can produce complex logarithms; invalid real special-function inputs report domain errors.",
    examples: LN_EXAMPLES,
    related: &["exp", "log", "log2", "log10"],
};

pub(super) const LOG2: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Log2,
    summary: "Return base-2 logarithms.",
    details: "`log2[xs]` broadcasts through nested numeric leaves, using the same complex and CAS behavior as `ln`.",
    examples: LOG2_EXAMPLES,
    related: &["log", "ln", "log10"],
};

pub(super) const LOG10: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Log10,
    summary: "Return base-10 logarithms.",
    details: "`log10[xs]` broadcasts through nested numeric leaves, using the same complex and CAS behavior as `ln`.",
    examples: LOG10_EXAMPLES,
    related: &["log", "ln", "log2"],
};

pub(super) const FLOOR: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Floor,
    summary: "Round values down.",
    details: "`floor[xs]` broadcasts and returns integral floors. `floor[x;d]` floors a real scalar to `d` decimal places, where `d` is an int.",
    examples: FLOOR_EXAMPLES,
    related: &["ceil", "round"],
};

pub(super) const CEIL: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Ceil,
    summary: "Round values up.",
    details: "`ceil[xs]` broadcasts and returns integral ceilings. `ceil[x;d]` ceils a real scalar to `d` decimal places, where `d` is an int.",
    examples: CEIL_EXAMPLES,
    related: &["floor", "round"],
};

pub(super) const ROUND: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Round,
    summary: "Round values to the nearest integer or decimal place.",
    details: "`round[xs]` broadcasts and returns integral rounded values. `round[x;d]` rounds a real scalar to `d` decimal places, where `d` is an int.",
    examples: ROUND_EXAMPLES,
    related: &["floor", "ceil"],
};

pub(super) const SIN: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Sin,
    summary: "Return sine values.",
    details: "`sin[xs]` broadcasts through nested numeric leaves. Inputs are in radians, and complex or CAS inputs follow the elementary-function path.",
    examples: SIN_EXAMPLES,
    related: &["cos", "tan", "arcsin"],
};

pub(super) const COS: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Cos,
    summary: "Return cosine values.",
    details: "`cos[xs]` broadcasts through nested numeric leaves. Inputs are in radians, and complex or CAS inputs follow the elementary-function path.",
    examples: COS_EXAMPLES,
    related: &["sin", "tan", "arccos"],
};

pub(super) const TAN: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Tan,
    summary: "Return tangent values.",
    details: "`tan[xs]` broadcasts through nested numeric leaves. Inputs are in radians, and complex or CAS inputs follow the elementary-function path.",
    examples: TAN_EXAMPLES,
    related: &["sin", "cos", "arctan"],
};

pub(super) const SEC: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Sec,
    summary: "Return secant values.",
    details: "`sec[xs]` broadcasts through nested numeric leaves and evaluates `1 / cos[x]`. Inputs are in radians.",
    examples: SEC_EXAMPLES,
    related: &["cos", "csc", "cot"],
};

pub(super) const CSC: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Csc,
    summary: "Return cosecant values.",
    details: "`csc[xs]` broadcasts through nested numeric leaves and evaluates `1 / sin[x]`. Inputs are in radians.",
    examples: CSC_EXAMPLES,
    related: &["sin", "sec", "cot"],
};

pub(super) const COT: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Cot,
    summary: "Return cotangent values.",
    details: "`cot[xs]` broadcasts through nested numeric leaves and evaluates `1 / tan[x]`. Inputs are in radians.",
    examples: COT_EXAMPLES,
    related: &["tan", "sec", "csc"],
};

pub(super) const ARCSIN: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Arcsin,
    summary: "Return inverse sine values.",
    details: "`arcsin[xs]` broadcasts through nested numeric leaves and returns radians. Real inputs outside the real domain can produce complex results.",
    examples: ARCSIN_EXAMPLES,
    related: &["sin", "arccos", "arctan"],
};

pub(super) const ARCCOS: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Arccos,
    summary: "Return inverse cosine values.",
    details: "`arccos[xs]` broadcasts through nested numeric leaves and returns radians. Real inputs outside the real domain can produce complex results.",
    examples: ARCCOS_EXAMPLES,
    related: &["cos", "arcsin", "arctan"],
};

pub(super) const ARCTAN: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Arctan,
    summary: "Return inverse tangent values.",
    details: "`arctan[xs]` broadcasts through nested numeric leaves and returns radians. Use `arctan2[y;x]` when the signs of both coordinates matter.",
    examples: ARCTAN_EXAMPLES,
    related: &["tan", "arctan2"],
};

pub(super) const SINH: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Sinh,
    summary: "Return hyperbolic sine values.",
    details: "`sinh[xs]` broadcasts through nested numeric leaves. Complex and CAS inputs follow the elementary-function path.",
    examples: SINH_EXAMPLES,
    related: &["cosh", "tanh", "arcsinh"],
};

pub(super) const COSH: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Cosh,
    summary: "Return hyperbolic cosine values.",
    details: "`cosh[xs]` broadcasts through nested numeric leaves. Complex and CAS inputs follow the elementary-function path.",
    examples: COSH_EXAMPLES,
    related: &["sinh", "tanh", "arccosh"],
};

pub(super) const TANH: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Tanh,
    summary: "Return hyperbolic tangent values.",
    details: "`tanh[xs]` broadcasts through nested numeric leaves. Complex and CAS inputs follow the elementary-function path.",
    examples: TANH_EXAMPLES,
    related: &["sinh", "cosh", "arctanh"],
};

pub(super) const ARCSINH: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Arcsinh,
    summary: "Return inverse hyperbolic sine values.",
    details: "`arcsinh[xs]` broadcasts through nested numeric leaves. Complex and CAS inputs follow the elementary-function path.",
    examples: ARCSINH_EXAMPLES,
    related: &["sinh", "arccosh", "arctanh"],
};

pub(super) const ARCCOSH: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Arccosh,
    summary: "Return inverse hyperbolic cosine values.",
    details: "`arccosh[xs]` broadcasts through nested numeric leaves. Real inputs below `1` can produce complex results.",
    examples: ARCCOSH_EXAMPLES,
    related: &["cosh", "arcsinh", "arctanh"],
};

pub(super) const ARCTANH: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Arctanh,
    summary: "Return inverse hyperbolic tangent values.",
    details: "`arctanh[xs]` broadcasts through nested numeric leaves. Real inputs outside `-1..1` can produce complex results.",
    examples: ARCTANH_EXAMPLES,
    related: &["tanh", "arcsinh", "arccosh"],
};

pub(super) const LOG: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Log,
    summary: "Return logarithms in a chosen base.",
    details: "`log[x;a]` returns the logarithm of `x` in base `a`, broadcasting compatible nested inputs. Complex and CAS inputs follow the elementary-function path.",
    examples: LOG_EXAMPLES,
    related: &["ln", "log2", "log10"],
};

pub(super) const ARCTAN2: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Arctan2,
    summary: "Return quadrant-aware inverse tangent values.",
    details: "`arctan2[y;x]` returns the angle, in radians, for the point `(x, y)`. It broadcasts compatible nested numeric inputs.",
    examples: ARCTAN2_EXAMPLES,
    related: &["arctan", "tan"],
};

pub(super) const ERF: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Erf,
    summary: "Return error-function values.",
    details: "`erf[xs]` broadcasts through nested real numeric leaves and returns the Gauss error function.",
    examples: ERF_EXAMPLES,
    related: &["erfc", "gamma"],
};

pub(super) const ERFC: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Erfc,
    summary: "Return complementary error-function values.",
    details: "`erfc[xs]` broadcasts through nested real numeric leaves and returns `1 - erf[x]` with the runtime's numeric implementation.",
    examples: ERFC_EXAMPLES,
    related: &["erf", "gamma"],
};

pub(super) const GAMMA: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Gamma,
    summary: "Return gamma-function values.",
    details: "`gamma[xs]` broadcasts through nested real numeric leaves and evaluates the extension of factorial, so `gamma 5` is `4!`.",
    examples: GAMMA_EXAMPLES,
    related: &["lngamma", "erf"],
};

pub(super) const LNGAMMA: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Lngamma,
    summary: "Return natural logs of gamma values.",
    details: "`lngamma[xs]` broadcasts through nested real numeric leaves and evaluates `ln[gamma[x]]` using the runtime's log-gamma implementation.",
    examples: LNGAMMA_EXAMPLES,
    related: &["gamma", "ln"],
};

pub(super) const SI: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Si,
    summary: "Return sine-integral values.",
    details: "`si[xs]` broadcasts through nested real numeric leaves and evaluates the sine integral `Si(x)`.",
    examples: SI_EXAMPLES,
    related: &["ci", "ei"],
};

pub(super) const CI: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Ci,
    summary: "Return cosine-integral values.",
    details: "`ci[xs]` broadcasts through nested real numeric leaves and evaluates the cosine integral `Ci(x)`.",
    examples: CI_EXAMPLES,
    related: &["si", "ei"],
};

pub(super) const EI: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Ei,
    summary: "Return exponential-integral values.",
    details: "`ei[xs]` broadcasts through nested real numeric leaves and evaluates the exponential integral `Ei(x)`.",
    examples: EI_EXAMPLES,
    related: &["si", "ci", "en"],
};

pub(super) const EN: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::En,
    summary: "Return generalized exponential-integral values.",
    details: "`en[n;xs]` evaluates the generalized exponential integral `E_n(x)`, broadcasting compatible real numeric inputs.",
    examples: EN_EXAMPLES,
    related: &["ei", "gamma"],
};

pub(super) const ELLPK: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Ellpk,
    summary: "Return complete elliptic integral K values.",
    details: "`ellpk[xs]` broadcasts through nested real numeric leaves and evaluates the Cephes-backed complete elliptic integral K with its `m1` parameter convention.",
    examples: ELLPK_EXAMPLES,
    related: &["ellpe", "ellik"],
};

pub(super) const ELLPE: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Ellpe,
    summary: "Return complete elliptic integral E values.",
    details: "`ellpe[xs]` broadcasts through nested real numeric leaves and evaluates the Cephes-backed complete elliptic integral E.",
    examples: ELLPE_EXAMPLES,
    related: &["ellpk", "ellie"],
};

pub(super) const ELLIK: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Ellik,
    summary: "Return incomplete elliptic integral K values.",
    details: "`ellik[phi;m]` evaluates the incomplete elliptic integral of the first kind for amplitude `phi` and parameter `m`, broadcasting compatible real numeric inputs.",
    examples: ELLIK_EXAMPLES,
    related: &["ellpk", "ellie"],
};

pub(super) const ELLIE: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Ellie,
    summary: "Return incomplete elliptic integral E values.",
    details: "`ellie[phi;m]` evaluates the incomplete elliptic integral of the second kind for amplitude `phi` and parameter `m`, broadcasting compatible real numeric inputs.",
    examples: ELLIE_EXAMPLES,
    related: &["ellpe", "ellik"],
};

pub(super) const HEAVISIDE: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Heaviside,
    summary: "Return Heaviside step values.",
    details: "`heaviside[xs]` broadcasts through nested real numeric leaves, returning `0.0` below zero, `0.5` at zero, and `1.0` above zero.",
    examples: HEAVISIDE_EXAMPLES,
    related: &["sgn", "delta"],
};

pub(super) const DELTA: BuiltinDoc = BuiltinDoc {
    builtin: BuiltinEnum::Delta,
    summary: "Return Dirac delta values away from zero.",
    details: "`delta[x]` accepts a real scalar, returns `0.0` for nonzero input, and reports a domain error at exactly zero because the ideal Dirac delta is singular there.",
    examples: DELTA_EXAMPLES,
    related: &["heaviside"],
};
