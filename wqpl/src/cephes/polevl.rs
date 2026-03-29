#![allow(clippy::excessive_precision)]

pub(super) const EUL: f64 = 0.57721566490153286061;

/// Evaluate polynomial using Horner's method.
/// Coefficients are in descending order:
/// `coef[0]*x^N + coef[1]*x^(N-1) + ... + coef[N]`
pub(super) fn polevl(x: f64, coef: &[f64]) -> f64 {
    let mut ans = coef[0];
    for &c in &coef[1..] {
        ans = ans * x + c;
    }
    ans
}

/// Evaluate polynomial where the leading coefficient of x^N is 1.0
/// and is omitted from the array.
pub(super) fn p1evl(x: f64, coef: &[f64]) -> f64 {
    let mut ans = x + coef[0];
    for &c in &coef[1..] {
        ans = ans * x + c;
    }
    ans
}
