use num_traits::ToPrimitive as _;

use crate::value::{Value, WqResult};
use crate::wqerror::{Requirement, WqError, WqErrorType};

/// The stable `wq-rng-v1` pseudo-random number generator.
///
/// Version 1 uses xoshiro256++ with SplitMix64 seed expansion. Its output is
/// part of wq's reproducibility contract and must not change.
#[derive(Clone, Debug)]
pub struct RngState {
    state: [u64; 4],
}

impl RngState {
    pub(crate) fn from_seed(seed: i64) -> Self {
        let mut seed = seed as u64;
        let state = std::array::from_fn(|_| splitmix64(&mut seed));
        Self { state }
    }

    pub(crate) fn from_entropy() -> Self {
        Self::from_seed(rand::random())
    }

    pub(crate) fn draw(&mut self, args: &[Value], source: &str, usage: &str) -> WqResult<Value> {
        match args {
            [] => Ok(Value::float(self.next_f64())),
            [upper] => {
                if let Some(upper) = signed_64_bit_int(upper)
                    && upper > 0
                {
                    return Ok(Value::Int(self.int_range(0, upper)));
                }
                if let Value::Float(upper) = upper
                    && upper.is_finite()
                    && **upper > 0.0
                {
                    return Ok(Value::float(self.float_range(0.0, **upper)));
                }
                Err(WqError::new(WqErrorType::Domain)
                    .src(source)
                    .expected(positive_random_bound_requirement())
                    .at_arg(0)
                    .got1(upper))
            }
            [lower, upper] => {
                if let (Some(lower), Some(upper)) =
                    (signed_64_bit_int(lower), signed_64_bit_int(upper))
                    && lower < upper
                {
                    return Ok(Value::Int(self.int_range(lower, upper)));
                }
                let lower = finite_bound(lower).ok_or_else(|| {
                    WqError::new(WqErrorType::Domain)
                        .src(source)
                        .expected(random_bound_requirement())
                        .at_arg(0)
                        .got1(lower)
                })?;
                let upper = finite_bound(upper).ok_or_else(|| {
                    WqError::new(WqErrorType::Domain)
                        .src(source)
                        .expected(random_bound_requirement())
                        .at_arg(1)
                        .got1(upper)
                })?;
                if lower < upper {
                    Ok(Value::float(self.float_range(lower, upper)))
                } else {
                    Err(WqError::new(WqErrorType::Domain)
                        .src(source)
                        .msg("lower bound must be less than upper bound")
                        .attach_note(format!("got lower bound {lower}"))
                        .attach_note(format!("got upper bound {upper}")))
                }
            }
            _ => Err(WqError::new(WqErrorType::Arity)
                .src(source)
                .msg(format!("expected 0, 1, or 2 arguments, got {}", args.len()))
                .attach_note(usage)),
        }
    }

    fn next_u64(&mut self) -> u64 {
        let result = self.state[0]
            .wrapping_add(self.state[3])
            .rotate_left(23)
            .wrapping_add(self.state[0]);
        let t = self.state[1] << 17;

        self.state[2] ^= self.state[0];
        self.state[3] ^= self.state[1];
        self.state[1] ^= self.state[2];
        self.state[0] ^= self.state[3];
        self.state[2] ^= t;
        self.state[3] = self.state[3].rotate_left(45);

        result
    }

    fn next_f64(&mut self) -> f64 {
        const SCALE: f64 = 1.0 / ((1_u64 << 53) as f64);
        (self.next_u64() >> 11) as f64 * SCALE
    }

    fn below(&mut self, upper: u64) -> u64 {
        debug_assert!(upper > 0);
        let threshold = upper.wrapping_neg() % upper;
        loop {
            let value = self.next_u64();
            if value >= threshold {
                return value % upper;
            }
        }
    }

    fn int_range(&mut self, lower: i64, upper: i64) -> i64 {
        let width = u64::try_from(i128::from(upper) - i128::from(lower))
            .expect("ordered i64 bounds have a positive width fitting u64");
        let offset = self.below(width);
        i64::try_from(i128::from(lower) + i128::from(offset))
            .expect("sampled offset remains inside i64 bounds")
    }

    fn float_range(&mut self, lower: f64, upper: f64) -> f64 {
        debug_assert!(lower.is_finite() && upper.is_finite() && lower < upper);
        let unit = self.next_f64();
        let width = upper - lower;
        let sampled = if width.is_finite() {
            lower + width * unit
        } else {
            lower * (1.0 - unit) + upper * unit
        };
        if sampled >= upper {
            upper.next_down()
        } else if sampled < lower {
            lower
        } else {
            sampled
        }
    }
}

fn signed_64_bit_int_requirement() -> Requirement {
    Requirement::phrase(
        "int in the signed 64-bit range",
        "ints in the signed 64-bit range",
    )
}

fn positive_random_bound_requirement() -> Requirement {
    Requirement::one_of([
        Requirement::positive(signed_64_bit_int_requirement()),
        Requirement::positive(Requirement::finite(Requirement::FLOAT)),
    ])
}

fn random_bound_requirement() -> Requirement {
    Requirement::one_of([
        signed_64_bit_int_requirement(),
        Requirement::finite(Requirement::FLOAT),
    ])
}

fn signed_64_bit_int(value: &Value) -> Option<i64> {
    match value {
        Value::Int(value) => Some(*value),
        Value::BigInt(value) => value.to_i64(),
        _ => None,
    }
}

fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
    let mut value = *state;
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

fn finite_bound(value: &Value) -> Option<f64> {
    match signed_64_bit_int(value) {
        Some(value) => Some(value as f64),
        None => match value {
            Value::Float(value) if value.is_finite() => Some(**value),
            _ => None,
        },
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use num_bigint::BigInt;

    use super::*;

    #[test]
    fn same_seed_repeats_raw_stream() {
        const EXPECTED: [u64; 16] = [
            15_021_278_609_987_233_951,
            5_881_210_131_331_364_753,
            18_149_643_915_985_481_100,
            12_933_668_939_759_105_464,
            14_637_574_242_682_825_331,
            10_848_501_901_068_131_965,
            2_312_344_417_745_909_078,
            11_162_538_943_635_311_430,
            3_831_705_504_650_218_695,
            17_217_215_411_128_672_468,
            10_321_681_451_779_520_834,
            15_680_282_660_304_795_149,
            12_543_905_331_768_826_776,
            1_282_610_804_685_344_189,
            7_435_390_023_275_438_269,
            10_071_993_084_810_367_336,
        ];
        let mut first = RngState::from_seed(42);
        let mut second = RngState::from_seed(42);
        for expected in EXPECTED {
            let value = first.next_u64();
            assert_eq!(value, expected);
            assert_eq!(value, second.next_u64());
        }
    }

    #[test]
    fn extreme_integer_bounds_stay_in_range() {
        let mut rng = RngState::from_seed(-1);
        for _ in 0..128 {
            let value = rng.int_range(i64::MIN, i64::MAX);
            assert!((i64::MIN..i64::MAX).contains(&value));
        }
    }

    #[test]
    fn wide_float_bounds_stay_half_open() {
        let mut rng = RngState::from_seed(123);
        for _ in 0..128 {
            let value = rng.float_range(-f64::MAX, f64::MAX);
            assert!((-f64::MAX..f64::MAX).contains(&value));
        }
    }

    #[test]
    fn draw_reports_composed_requirements_and_argument_positions() {
        let mut rng = RngState::from_seed(0);
        let error = rng
            .draw(&[Value::Int(0)], "rng", "rng[]")
            .expect_err("zero upper bound should fail");

        assert_eq!(
            error.msg.as_deref(),
            Some("expected positive int in the signed 64-bit range or positive finite float")
        );
        assert_eq!(error.notes.as_slice(), ["at argument 1", "got 0 (int)"]);
    }

    #[test]
    fn draw_reports_the_machine_int_boundary_for_bigints() {
        let mut rng = RngState::from_seed(0);
        let fitting = Value::BigInt(Arc::new(BigInt::from(10)));
        let sampled = rng
            .draw(&[fitting], "rng", "rng[]")
            .expect("a bigint in the signed 64-bit range should work");
        assert!(matches!(sampled, Value::Int(value) if (0..10).contains(&value)));

        let too_large = Value::BigInt(Arc::new(BigInt::from(i64::MAX) + 1));
        let error = rng
            .draw(&[too_large], "rng", "rng[]")
            .expect_err("a bigint outside the signed 64-bit range should fail");

        assert_eq!(
            error.msg.as_deref(),
            Some("expected positive int in the signed 64-bit range or positive finite float")
        );
        assert_eq!(
            error.notes.as_slice(),
            ["at argument 1", "got 9223372036854775808 (int)",]
        );

        let too_large = Value::BigInt(Arc::new(BigInt::from(i64::MAX) + 1));
        let error = rng
            .draw(&[too_large, Value::Int(0)], "rng", "rng[]")
            .expect_err("an out-of-range bigint lower bound should fail");
        assert_eq!(
            error.msg.as_deref(),
            Some("expected int in the signed 64-bit range or finite float")
        );
        assert_eq!(
            error.notes.as_slice(),
            ["at argument 1", "got 9223372036854775808 (int)",]
        );
    }
}
