use std::sync::Arc;

use encoding_rs::Encoding;

use crate::builtins::{BuiltinEnum as BE, BuiltinFnArgs, check_arity};
use crate::value::{IntoWqValue as _, Value, WqResult, expected_bytes1, expected_string1};
use crate::wqerror::{Requirement, WqError, WqErrorType};

fn find_encoding(label: &str) -> Option<&'static Encoding> {
    Encoding::for_label(label.as_bytes())
}

pub(super) fn decode(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::Decode, [2, 3], &args)?;
    let (bytes, codec, mode) = match &*args {
        [bytes, codec] => {
            let bytes = bytes
                .try_to_rust_vec_u8()
                .ok_or_else(|| expected_bytes1(bytes).src(BE::Decode).at_arg(0))?;
            let codec = codec
                .try_to_rust_string()
                .ok_or_else(|| expected_string1(codec).src(BE::Decode).at_arg(1))?;
            (bytes, codec, "s".to_string())
        }
        [bytes, codec, mode] => {
            let bytes = bytes
                .try_to_rust_vec_u8()
                .ok_or_else(|| expected_bytes1(bytes).src(BE::Decode).at_arg(0))?;
            let codec = codec
                .try_to_rust_string()
                .ok_or_else(|| expected_string1(codec).src(BE::Decode).at_arg(1))?;
            let mode = mode
                .try_to_rust_string()
                .ok_or_else(|| expected_string1(mode).src(BE::Decode).at_arg(2))?;
            (bytes, codec, mode)
        }
        _ => unreachable!(),
    };

    let enc = find_encoding(&codec).ok_or_else(|| {
        WqError::new(WqErrorType::Encode)
            .src(BE::Decode)
            .msg(format!("unsupported codec \"{codec}\""))
    })?;
    let (text, had_errors) = enc.decode_without_bom_handling(&bytes);
    let s = match mode.as_str() {
        "s" => {
            if had_errors {
                return Err(WqError::new(WqErrorType::Encode)
                    .src(BE::Decode)
                    .msg("strict mode decode error"));
            }
            text
        }
        "r" => text,
        _ => {
            return Err(WqError::new(WqErrorType::Domain)
                .src(BE::Decode)
                .expected(Requirement::one_of([
                    Requirement::string_literal("s"),
                    Requirement::string_literal("r"),
                ]))
                .at_arg(2)
                .attach_note("\"s\" selects strict mode; \"r\" selects replacement mode"));
        }
    };
    Ok(s.into_wq_value())
}

pub(super) fn encode(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::Encode, [2, 3], &args)?;
    let (text, codec, mode) = match &*args {
        [text, codec] => {
            let text = text
                .try_to_rust_string()
                .ok_or_else(|| expected_string1(text).src(BE::Encode).at_arg(0))?;
            let codec = codec
                .try_to_rust_string()
                .ok_or_else(|| expected_string1(codec).src(BE::Encode).at_arg(1))?;

            (text, codec, "s".to_string())
        }
        [text, codec, mode] => {
            let text = text
                .try_to_rust_string()
                .ok_or_else(|| expected_string1(text).src(BE::Encode).at_arg(0))?;
            let codec = codec
                .try_to_rust_string()
                .ok_or_else(|| expected_string1(codec).src(BE::Encode).at_arg(1))?;
            let mode = mode
                .try_to_rust_string()
                .ok_or_else(|| expected_string1(mode).src(BE::Encode).at_arg(2))?;
            (text, codec, mode)
        }
        _ => unreachable!(),
    };
    let enc = find_encoding(&codec).ok_or_else(|| {
        WqError::new(WqErrorType::Encode)
            .src(BE::Encode)
            .msg(format!("unsupported codec \"{codec}\""))
    })?;
    let out: Vec<u8> = match mode.as_str() {
        "s" => {
            let (cow, _enc_used, had_errors) = enc.encode(&text);
            if had_errors {
                return Err(WqError::new(WqErrorType::Encode)
                    .src(BE::Encode)
                    .msg("strict mode encode error"));
            }
            cow.into_owned()
        }
        "r" => {
            let (cow, _enc_used, _had_errors) = enc.encode(&text);
            cow.into_owned()
        }
        _ => {
            return Err(WqError::new(WqErrorType::Domain)
                .src(BE::Encode)
                .expected(Requirement::one_of([
                    Requirement::string_literal("s"),
                    Requirement::string_literal("r"),
                ]))
                .at_arg(2)
                .attach_note("\"s\" selects strict mode; \"r\" selects replacement mode"));
        }
    };
    Ok(Value::IntList(Arc::new(
        out.into_iter().map(|b| b.into()).collect(),
    )))
}

pub(super) fn is_valid_bytes(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::ValidBytes, [1], &args)?;
    Ok(Value::Bool(args[0].can_convert_to_vec_u8()))
}

#[cfg(test)]
mod tests {
    use smallvec::smallvec;

    use super::*;

    #[test]
    fn encode_decode_roundtrip() {
        let s = "héllo".into_wq_value();
        let b = encode(BuiltinFnArgs::from(smallvec![
            s.clone(),
            "utf-8".into_wq_value()
        ]))
        .unwrap();
        let d = decode(BuiltinFnArgs::from(smallvec![b, "utf-8".into_wq_value()])).unwrap();
        assert_eq!(d, s);
    }

    #[test]
    fn encode_and_decode_report_canonical_mode_values() {
        let invalid_mode = "invalid".into_wq_value();
        let encode_error = encode(BuiltinFnArgs::from(smallvec![
            "x".into_wq_value(),
            "utf-8".into_wq_value(),
            invalid_mode.clone(),
        ]))
        .expect_err("invalid encode mode should fail");
        let decode_error = decode(BuiltinFnArgs::from(smallvec![
            Value::Int(65),
            "utf-8".into_wq_value(),
            invalid_mode,
        ]))
        .expect_err("invalid decode mode should fail");

        assert_eq!(encode_error.msg.as_deref(), Some("expected \"s\" or \"r\""));
        assert_eq!(decode_error.msg.as_deref(), Some("expected \"s\" or \"r\""));
        assert_eq!(
            encode_error.notes.as_slice(),
            [
                "at argument 3",
                "\"s\" selects strict mode; \"r\" selects replacement mode"
            ]
        );
        assert_eq!(decode_error.notes, encode_error.notes);
    }
}
