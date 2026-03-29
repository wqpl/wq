//! Complete and incomplete elliptic integrals.
//! Ported from Cephes.

#![allow(clippy::excessive_precision)]

use super::polevl::polevl;

// ─────────────────────────────────────────────────────────────────────────────
// Complete elliptic integral of the first kind K(m1)
// ─────────────────────────────────────────────────────────────────────────────

#[rustfmt::skip]
static ELLPK_P: &[f64] = &[
    1.37982864606273237150E-4,
    2.28025724005875567385E-3,
    7.97404013220415179367E-3,
    9.85821379021226008714E-3,
    6.87489687449949877925E-3,
    6.18901033637687613229E-3,
    8.79078273952743772254E-3,
    1.49380448916805252718E-2,
    3.08851465246711995998E-2,
    9.65735902811690126535E-2,
    1.38629436111989062502E0,
];
#[rustfmt::skip]
static ELLPK_Q: &[f64] = &[
    2.94078955048598507511E-5,
    9.14184723865917226571E-4,
    5.94058303753167793257E-3,
    1.54850516649762399335E-2,
    2.39089602715924892727E-2,
    3.01204715227604046988E-2,
    3.73774314173823228969E-2,
    4.88280347570998239232E-2,
    7.03124996963957469739E-2,
    1.24999999999870820058E-1,
    4.99999999999999999821E-1,
];

/// Complete elliptic integral of the first kind.
///
/// `ellpk(m1) = K(m)` where `m = 1 - m1`.
///
/// `K(m) = ∫₀^(π/2) (1 - m sin²t)^(-1/2) dt`
///
/// Domain: `0 ≤ m1 ≤ 1`.
pub fn ellpk(m1: f64) -> f64 {
    if !(0.0..=1.0).contains(&m1) {
        return f64::NAN;
    }
    if m1 > f64::EPSILON {
        polevl(m1, ELLPK_P) - m1.ln() * polevl(m1, ELLPK_Q)
    } else if m1 == 0.0 {
        f64::INFINITY
    } else {
        4.0_f64.ln() - 0.5 * m1.ln()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Complete elliptic integral of the second kind E(m1)
// ─────────────────────────────────────────────────────────────────────────────

#[rustfmt::skip]
static ELLPE_P: &[f64] = &[
    1.53552577301013293365E-4,
    2.50888492163602060990E-3,
    8.68786816565889628429E-3,
    1.07350949056076193403E-2,
    7.77395492516787092951E-3,
    7.58395289413514708519E-3,
    1.15688436810574127319E-2,
    2.18317996015557253103E-2,
    5.68051945617860553470E-2,
    4.43147180560990850618E-1,
    1.00000000000000000299E0,
];
#[rustfmt::skip]
static ELLPE_Q: &[f64] = &[
    3.27954898576485872656E-5,
    1.00962792679356715133E-3,
    6.50609489976927491433E-3,
    1.68862163993311317300E-2,
    2.61769742454493659583E-2,
    3.34833904888224918614E-2,
    4.27180926518931511717E-2,
    5.85936634471101055642E-2,
    9.37499997197644278445E-2,
    2.49999999999888314361E-1,
];

/// Complete elliptic integral of the second kind.
///
/// `ellpe(m1) = E(m)` where `m = 1 - m1`.
///
/// `E(m) = ∫₀^(π/2) sqrt(1 - m sin²t) dt`
///
/// Domain: `0 ≤ m1 ≤ 1`.
pub fn ellpe(m1: f64) -> f64 {
    if m1 == 0.0 {
        return 1.0;
    }
    if !(0.0..=1.0).contains(&m1) {
        return f64::NAN;
    }
    polevl(m1, ELLPE_P) - m1.ln() * (m1 * polevl(m1, ELLPE_Q))
}

// ─────────────────────────────────────────────────────────────────────────────
// Incomplete elliptic integral of the first kind F(phi, m)
// ─────────────────────────────────────────────────────────────────────────────

/// Incomplete elliptic integral of the first kind.
///
/// `ellik(phi, m) = F(φ | m) = ∫₀^φ (1 - m sin²t)^(-1/2) dt`
///
/// Domain: `0 ≤ m ≤ 1`.
pub fn ellik(phi: f64, m: f64) -> f64 {
    if !(0.0..=1.0).contains(&m) {
        return f64::NAN;
    }
    if m == 0.0 {
        return phi;
    }
    let a0 = 1.0 - m;
    if a0 == 0.0 {
        if phi.abs() >= std::f64::consts::FRAC_PI_2 {
            return f64::INFINITY;
        }
        return ((std::f64::consts::FRAC_PI_2 + phi) / 2.0).tan().ln();
    }

    let mut npio2 = (phi / std::f64::consts::FRAC_PI_2).floor() as i32;
    if npio2 & 1 != 0 {
        npio2 += 1;
    }

    let mut k = 0.0;
    let mut phi = phi;
    if npio2 != 0 {
        k = ellpk(a0);
        phi -= npio2 as f64 * std::f64::consts::FRAC_PI_2;
    }

    let sign = if phi < 0.0 {
        phi = -phi;
        -1
    } else {
        0
    };

    let mut b = a0.sqrt();
    let mut t = phi.tan();

    let mut temp;
    if t.abs() > 10.0 {
        let e = 1.0 / (b * t);
        if e.abs() < 10.0 {
            let e = e.atan();
            if npio2 == 0 {
                k = ellpk(a0);
            }
            temp = k - ellik(e, m);
            if sign < 0 {
                temp = -temp;
            }
            temp += npio2 as f64 * k;
            return temp;
        }
    }

    let mut a = 1.0;
    let mut c = m.sqrt();
    let mut d = 1;
    let mut mod_val = 0;

    while (c / a).abs() > f64::EPSILON {
        let temp_ratio = b / a;
        phi = phi + (t * temp_ratio).atan() + mod_val as f64 * std::f64::consts::PI;
        mod_val = ((phi + std::f64::consts::FRAC_PI_2) / std::f64::consts::PI).floor() as i32;
        t *= (1.0 + temp_ratio) / (1.0 - temp_ratio * t * t);
        c = (a - b) / 2.0;
        let temp_sqrt = (a * b).sqrt();
        a = (a + b) / 2.0;
        b = temp_sqrt;
        d += d;
    }

    temp = (t.atan() + mod_val as f64 * std::f64::consts::PI) / (d as f64 * a);

    if sign < 0 {
        temp = -temp;
    }
    temp += npio2 as f64 * k;
    temp
}

// ─────────────────────────────────────────────────────────────────────────────
// Incomplete elliptic integral of the second kind E(phi, m)
// ─────────────────────────────────────────────────────────────────────────────

/// Incomplete elliptic integral of the second kind.
///
/// `ellie(phi, m) = E(φ | m) = ∫₀^φ sqrt(1 - m sin²t) dt`
///
/// Domain: `0 ≤ m ≤ 1`.
pub fn ellie(phi: f64, m: f64) -> f64 {
    if !(0.0..=1.0).contains(&m) {
        return f64::NAN;
    }
    if m == 0.0 {
        return phi;
    }

    let mut lphi = phi;
    let mut npio2 = (lphi / std::f64::consts::FRAC_PI_2).floor() as i32;
    if npio2 & 1 != 0 {
        npio2 += 1;
    }
    lphi -= npio2 as f64 * std::f64::consts::FRAC_PI_2;

    let (sign, mut lphi) = if lphi < 0.0 { (-1, -lphi) } else { (1, lphi) };

    let a0 = 1.0 - m;
    let e_val = ellpe(a0);
    if a0 == 0.0 {
        let mut temp = lphi.sin();
        if sign < 0 {
            temp = -temp;
        }
        temp += npio2 as f64 * e_val;
        return temp;
    }

    let mut t = lphi.tan();
    let mut b = a0.sqrt();

    let mut temp;
    if t.abs() > 10.0 {
        let e = 1.0 / (b * t);
        if e.abs() < 10.0 {
            let e = e.atan();
            temp = e_val + m * lphi.sin() * e.sin() - ellie(e, m);
            if sign < 0 {
                temp = -temp;
            }
            temp += npio2 as f64 * e_val;
            return temp;
        }
    }

    let mut a = 1.0;
    let mut c = m.sqrt();
    let mut d = 1;
    let mut e_acc = 0.0;
    let mut mod_val = 0;

    while (c / a).abs() > f64::EPSILON {
        let temp_ratio = b / a;
        lphi = lphi + (t * temp_ratio).atan() + mod_val as f64 * std::f64::consts::PI;
        mod_val = ((lphi + std::f64::consts::FRAC_PI_2) / std::f64::consts::PI).floor() as i32;
        t *= (1.0 + temp_ratio) / (1.0 - temp_ratio * t * t);
        c = (a - b) / 2.0;
        let temp_sqrt = (a * b).sqrt();
        a = (a + b) / 2.0;
        d += d;
        e_acc += c * lphi.sin();
        b = temp_sqrt;
    }

    temp = e_val / ellpk(a0);
    temp *= (t.atan() + mod_val as f64 * std::f64::consts::PI) / (d as f64 * a);
    temp += e_acc;

    if sign < 0 {
        temp = -temp;
    }
    temp += npio2 as f64 * e_val;
    temp
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ellpk_known_values() {
        assert!((ellpk(1.0) - std::f64::consts::FRAC_PI_2).abs() < 1e-14);
        assert!((ellpk(0.75) - 1.6857503548125960).abs() < 1e-14);
        assert!((ellpk(0.5) - 1.8540746773013719).abs() < 1e-14);
        assert!((ellpk(0.25) - 2.1565156474996432).abs() < 1e-14);
        assert!((ellpk(0.1) - 2.5780921133481733).abs() < 1e-14);
        assert!((ellpk(0.01) - 3.6956373629898742).abs() < 1e-14);
        assert!(ellpk(0.0).is_infinite());
        assert!(ellpk(-0.1).is_nan());
        assert!(ellpk(1.1).is_nan());
    }

    #[test]
    fn ellpe_known_values() {
        assert!((ellpe(1.0) - std::f64::consts::FRAC_PI_2).abs() < 1e-14);
        assert!((ellpe(0.75) - 1.4674622093394272).abs() < 1e-14);
        assert!((ellpe(0.5) - 1.3506438810476755).abs() < 1e-14);
        assert!((ellpe(0.25) - 1.2110560275684595).abs() < 1e-14);
        assert!((ellpe(0.1) - 1.1047747327040733).abs() < 1e-14);
        assert!((ellpe(0.01) - 1.0159935450252239).abs() < 1e-14);
        assert!((ellpe(0.0) - 1.0).abs() < 1e-14);
        assert!(ellpe(-0.1).is_nan());
        assert!(ellpe(1.1).is_nan());
    }

    #[test]
    fn ellik_known_values() {
        assert!(ellik(0.0, 0.5).abs() < 1e-14);
        assert!((ellik(0.5, 0.5) - 0.5104671356280048).abs() < 1e-14);
        assert!((ellik(1.0, 0.5) - 1.0832167728451688).abs() < 1e-14);
        assert!((ellik(1.5, 0.3) - 1.6293018873244832).abs() < 1e-14);
        assert!((ellik(2.0, 0.3) - 2.2205905521284742).abs() < 1e-14);
        assert!((ellik(0.7, 0.9) - 0.7572025472958656).abs() < 1e-14);
        assert!((ellik(3.0, 0.1) - 3.0832428789097927).abs() < 1e-14);
        // Special cases
        assert!((ellik(std::f64::consts::FRAC_PI_2, 0.5) - ellpk(0.5)).abs() < 1e-14);
        assert!((ellik(-1.0, 0.5) + 1.0832167728451688).abs() < 1e-14);
    }

    #[test]
    fn ellie_known_values() {
        assert!(ellie(0.0, 0.5).abs() < 1e-14);
        assert!((ellie(0.5, 0.5) - 0.48991095979251716).abs() < 1e-14);
        assert!((ellie(1.0, 0.5) - 0.9273298836244401).abs() < 1e-14);
        assert!((ellie(1.5, 0.3) - 1.3861094300893470).abs() < 1e-14);
        assert!((ellie(2.0, 0.3) - 1.8089647253633312).abs() < 1e-14);
        assert!((ellie(0.7, 0.9) - 0.6502171629260443).abs() < 1e-14);
        assert!((ellie(3.0, 0.1) - 2.9199697569208234).abs() < 1e-14);
        // Special cases
        assert!((ellie(std::f64::consts::FRAC_PI_2, 0.5) - ellpe(0.5)).abs() < 1e-14);
        assert!((ellie(-1.0, 0.5) + 0.9273298836244401).abs() < 1e-14);
    }
}
