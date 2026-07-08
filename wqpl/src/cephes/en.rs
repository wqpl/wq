//! Exponential integral of order n, En(n, x).
//! Ported from `cephes/misc/expn.c`.

#![allow(clippy::excessive_precision)]

use super::polevl::EUL;

const BIG: f64 = 1.44115188075855872e17;
const MACHEP: f64 = f64::EPSILON;
const MAXLOG: f64 = 709.782712893384; // approximately f64::MAX.ln()

/// Exponential integral En(n, x) for n >= 0, x >= 0.
///
/// E_n(x) = integral from 1 to infinity of e^(-xt) / t^n dt
///
/// Returns NaN for n < 0 or x < 0.
pub fn en(n: i32, x: f64) -> f64 {
    if n < 0 || x < 0.0 {
        return f64::NAN;
    }
    if x > MAXLOG {
        return 0.0;
    }
    if x == 0.0 {
        if n < 2 {
            return f64::INFINITY;
        }
        return 1.0 / (n as f64 - 1.0);
    }
    if n == 0 {
        return (-x).exp() / x;
    }

    // Large-n asymptotic expansion
    if n > 5000 {
        let xk = x + n as f64;
        let yk = 1.0 / (xk * xk);
        let t = n as f64;
        let mut ans = yk * t * (6.0 * x * x - 8.0 * t * x + t * t);
        ans = yk * (ans + t * (t - 2.0 * x));
        ans = yk * (ans + t);
        ans = (ans + 1.0) * (-x).exp() / xk;
        return ans;
    }

    if x > 1.0 {
        // Continued fraction
        return en_cfrac(n, x);
    }

    // Power series expansion
    en_power_series(n, x)
}

fn en_power_series(n: i32, x: f64) -> f64 {
    let mut psi = -EUL - x.ln();
    for i in 1..n {
        psi += 1.0 / i as f64;
    }

    let z = -x;
    let mut xk = 0.0;
    let mut yk = 1.0;
    let mut pk = 1.0 - n as f64;
    let mut ans = if n == 1 { 0.0 } else { 1.0 / pk };
    let mut t: f64;

    loop {
        xk += 1.0;
        yk *= z / xk;
        pk += 1.0;
        if pk != 0.0 {
            ans += yk / pk;
        }
        t = if ans != 0.0 { (yk / ans).abs() } else { 1.0 };
        if t <= MACHEP {
            break;
        }
    }

    let r = n as f64 - 1.0;
    (z.powf(r) * psi / libm::tgamma(n as f64)) - ans
}

fn en_cfrac(n: i32, x: f64) -> f64 {
    let mut k = 1;
    let mut pkm2 = 1.0;
    let mut qkm2 = x;
    let mut pkm1 = 1.0;
    let mut qkm1 = x + n as f64;
    let mut ans = pkm1 / qkm1;
    let mut t: f64;
    let big = BIG;

    loop {
        k += 1;
        let (yk, xk): (f64, f64);
        if k & 1 != 0 {
            yk = 1.0;
            xk = n as f64 + ((k - 1) / 2) as f64;
        } else {
            yk = x;
            xk = (k / 2) as f64;
        }
        let pk = pkm1 * yk + pkm2 * xk;
        let qk = qkm1 * yk + qkm2 * xk;
        if qk != 0.0 {
            let r = pk / qk;
            t = ((ans - r) / r).abs();
            ans = r;
        } else {
            t = 1.0;
        }
        pkm2 = pkm1;
        pkm1 = pk;
        qkm2 = qkm1;
        qkm1 = qk;
        if pk.abs() > big {
            pkm2 /= big;
            pkm1 /= big;
            qkm2 /= big;
            qkm1 /= big;
        }
        if t <= MACHEP {
            break;
        }
    }

    ans * (-x).exp()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn en_known_values() {
        assert!(en(0, 1.0) - (-1.0_f64).exp() / 1.0 < 1e-15);
        assert!((en(1, 1.0) - 0.21938393439552029).abs() < 1e-14);
        assert!((en(2, 1.0) - 0.14849550677592205).abs() < 1e-14);
        assert!(en(2, 0.0) - 1.0 < 1e-15);
        assert!(en(1, 0.0).is_infinite());
    }
}
