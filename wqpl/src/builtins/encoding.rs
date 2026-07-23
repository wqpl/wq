use std::sync::Arc;

use encoding_rs::{CoderResult, DecoderResult, Encoding};

use crate::builtins::{BuiltinEnum as BE, BuiltinFnArgs, check_arity, check_registered_args};
use crate::value::{IntoWqValue as _, Value, WqResult, expected_bytes1, expected_string1};
use crate::wqerror::{Requirement, WqError, WqErrorType};

fn find_encoding(label: &str) -> Option<&'static Encoding> {
    Encoding::for_label(label.as_bytes())
}

#[derive(Clone, Copy)]
enum ErrorMode {
    Strict,
    Replace,
}

#[derive(Clone, Copy)]
enum BomPolicy {
    Preserve,
    Strip,
}

pub(super) fn decode(args: BuiltinFnArgs) -> WqResult<Value> {
    check_registered_args(BE::Decode, &args)?;
    let bytes = args[0]
        .try_to_rust_vec_u8()
        .ok_or_else(|| expected_bytes1(&args[0]).src(BE::Decode).at_arg(0))?;
    let codec = args[1]
        .try_to_rust_string()
        .ok_or_else(|| expected_string1(&args[1]).src(BE::Decode).at_arg(1))?;
    let mode = parse_mode(&args, BE::Decode)?;
    let bom = parse_bom_policy(&args)?;
    let encoding = find_encoding(&codec).ok_or_else(|| {
        WqError::new(WqErrorType::Encode)
            .src(BE::Decode)
            .msg(format!("unsupported codec \"{codec}\""))
    })?;
    let text = match mode {
        ErrorMode::Strict => strict_decode(encoding, &bytes, bom, &codec)?,
        ErrorMode::Replace => replacement_decode(encoding, &bytes, bom),
    };
    Ok(text.into_wq_value())
}

pub(super) fn encode(args: BuiltinFnArgs) -> WqResult<Value> {
    check_registered_args(BE::Encode, &args)?;
    let text = args[0]
        .try_to_rust_string()
        .ok_or_else(|| expected_string1(&args[0]).src(BE::Encode).at_arg(0))?;
    let codec = args[1]
        .try_to_rust_string()
        .ok_or_else(|| expected_string1(&args[1]).src(BE::Encode).at_arg(1))?;
    let mode = parse_mode(&args, BE::Encode)?;
    let encoding = find_encoding(&codec).ok_or_else(|| {
        WqError::new(WqErrorType::Encode)
            .src(BE::Encode)
            .msg(format!("unsupported codec \"{codec}\""))
    })?;
    let (output, _, had_errors) = encoding.encode(&text);
    if matches!(mode, ErrorMode::Strict) && had_errors {
        for (offset, character) in text.chars().enumerate() {
            if encoding.encode(&character.to_string()).2 {
                return Err(unrepresentable_character(&codec, offset));
            }
        }
        return Err(WqError::new(WqErrorType::Encode)
            .src(BE::Encode)
            .msg(format!("string cannot be represented by codec \"{codec}\"")));
    }
    Ok(Value::IntList(Arc::new(
        output.into_owned().into_iter().map(i64::from).collect(),
    )))
}

fn unrepresentable_character(codec: &str, offset: usize) -> WqError {
    WqError::new(WqErrorType::Encode)
        .src(BE::Encode)
        .msg(format!(
            "character cannot be represented by codec \"{codec}\" at character offset {offset}"
        ))
}

fn parse_mode(args: &BuiltinFnArgs, source: BE) -> WqResult<ErrorMode> {
    match args.named("mode") {
        None => Ok(ErrorMode::Strict),
        Some(Value::Tag(mode)) if mode.as_ref() == "strict" => Ok(ErrorMode::Strict),
        Some(Value::Tag(mode)) if mode.as_ref() == "replace" => Ok(ErrorMode::Replace),
        Some(value) => Err(WqError::new(WqErrorType::Domain)
            .src(source)
            .expected(Requirement::one_of([
                Requirement::literal("`strict"),
                Requirement::literal("`replace"),
            ]))
            .at_named_arg("mode")
            .got1(value)),
    }
}

fn parse_bom_policy(args: &BuiltinFnArgs) -> WqResult<BomPolicy> {
    match args.named("bom") {
        None => Ok(BomPolicy::Preserve),
        Some(Value::Tag(policy)) if policy.as_ref() == "preserve" => Ok(BomPolicy::Preserve),
        Some(Value::Tag(policy)) if policy.as_ref() == "strip" => Ok(BomPolicy::Strip),
        Some(value) => Err(WqError::new(WqErrorType::Domain)
            .src(BE::Decode)
            .expected(Requirement::one_of([
                Requirement::literal("`preserve"),
                Requirement::literal("`strip"),
            ]))
            .at_named_arg("bom")
            .got1(value)),
    }
}

fn decoder_for(encoding: &'static Encoding, bom: BomPolicy) -> encoding_rs::Decoder {
    match bom {
        BomPolicy::Preserve => encoding.new_decoder_without_bom_handling(),
        BomPolicy::Strip => encoding.new_decoder_with_bom_removal(),
    }
}

fn output_capacity(byte_count: usize) -> usize {
    byte_count.saturating_mul(3).saturating_add(16)
}

fn replacement_decode(encoding: &'static Encoding, bytes: &[u8], bom: BomPolicy) -> String {
    let mut decoder = decoder_for(encoding, bom);
    let mut output = String::with_capacity(output_capacity(bytes.len()));
    let mut consumed = 0;
    loop {
        let (result, read, _) = decoder.decode_to_string(&bytes[consumed..], &mut output, true);
        consumed += read;
        match result {
            CoderResult::InputEmpty => return output,
            CoderResult::OutputFull => output.reserve(output.capacity().max(16)),
        }
    }
}

fn strict_decode(
    encoding: &'static Encoding,
    bytes: &[u8],
    bom: BomPolicy,
    codec: &str,
) -> WqResult<String> {
    let mut decoder = decoder_for(encoding, bom);
    let mut output = String::with_capacity(output_capacity(bytes.len()));
    let mut consumed = 0;
    loop {
        let (result, read) =
            decoder.decode_to_string_without_replacement(&bytes[consumed..], &mut output, true);
        consumed += read;
        match result {
            DecoderResult::InputEmpty => return Ok(output),
            DecoderResult::OutputFull => output.reserve(output.capacity().max(16)),
            DecoderResult::Malformed(malformed_length, bytes_after_malformed) => {
                let offset = consumed.saturating_sub(
                    usize::from(malformed_length) + usize::from(bytes_after_malformed),
                );
                let kind = if decoder_accepts_prefix(encoding, bytes, bom) {
                    "incomplete final byte sequence"
                } else {
                    "malformed byte sequence"
                };
                return Err(WqError::new(WqErrorType::Encode)
                    .src(BE::Decode)
                    .msg(format!(
                        "{kind} for codec \"{codec}\" at byte offset {offset}"
                    )));
            }
        }
    }
}

fn decoder_accepts_prefix(encoding: &'static Encoding, bytes: &[u8], bom: BomPolicy) -> bool {
    let mut decoder = decoder_for(encoding, bom);
    let mut output = String::with_capacity(output_capacity(bytes.len()));
    let mut consumed = 0;
    loop {
        let (result, read) =
            decoder.decode_to_string_without_replacement(&bytes[consumed..], &mut output, false);
        consumed += read;
        match result {
            DecoderResult::InputEmpty => return true,
            DecoderResult::Malformed(_, _) => return false,
            DecoderResult::OutputFull => output.reserve(output.capacity().max(16)),
        }
    }
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
        .expect("UTF-8 encoding should succeed");
        let d = decode(BuiltinFnArgs::from(smallvec![b, "utf-8".into_wq_value()]))
            .expect("UTF-8 decoding should succeed");
        assert_eq!(d, s);
    }

    #[test]
    fn decode_named_replacement_mode_and_bom_policy() {
        let replacement = decode(BuiltinFnArgs::with_named(
            smallvec![
                Value::IntList(Arc::new(vec![0x66, 0x80, 0x6f])),
                "utf-8".into_wq_value(),
            ],
            vec![(Arc::from("mode"), Value::Tag(Arc::from("replace")))],
        ))
        .expect("replacement mode should decode malformed bytes");
        assert_eq!(replacement, "f�o".into_wq_value());

        let bytes = Value::IntList(Arc::new(vec![0xef, 0xbb, 0xbf, 0x61]));
        let preserved = decode(BuiltinFnArgs::from(smallvec![
            bytes.clone(),
            "utf-8".into_wq_value(),
        ]))
        .expect("default BOM preservation should decode");
        let stripped = decode(BuiltinFnArgs::with_named(
            smallvec![bytes, "utf-8".into_wq_value()],
            vec![(Arc::from("bom"), Value::Tag(Arc::from("strip")))],
        ))
        .expect("BOM stripping should decode");
        assert_eq!(preserved, "\u{feff}a".into_wq_value());
        assert_eq!(stripped, "a".into_wq_value());
    }

    #[test]
    fn strict_decode_reports_malformed_and_incomplete_offsets() {
        let malformed = decode(BuiltinFnArgs::from(smallvec![
            Value::IntList(Arc::new(vec![0x61, 0xff])),
            "utf-8".into_wq_value(),
        ]))
        .expect_err("malformed UTF-8 should fail");
        let incomplete = decode(BuiltinFnArgs::from(smallvec![
            Value::IntList(Arc::new(vec![0x61, 0xc3])),
            "utf-8".into_wq_value(),
        ]))
        .expect_err("incomplete UTF-8 should fail");

        assert_eq!(
            malformed.msg.as_deref(),
            Some("malformed byte sequence for codec \"utf-8\" at byte offset 1")
        );
        assert_eq!(
            incomplete.msg.as_deref(),
            Some("incomplete final byte sequence for codec \"utf-8\" at byte offset 1")
        );
    }

    #[test]
    fn encode_uses_named_strict_and_replacement_modes() {
        let strict = encode(BuiltinFnArgs::from(smallvec![
            "a🦀".into_wq_value(),
            "windows-1252".into_wq_value(),
        ]))
        .expect_err("strict encoding should reject unrepresentable chars");
        assert!(
            strict
                .msg
                .as_deref()
                .is_some_and(|message| message.contains("character offset 1"))
        );

        let replacement = encode(BuiltinFnArgs::with_named(
            smallvec!["a🦀".into_wq_value(), "windows-1252".into_wq_value(),],
            vec![(Arc::from("mode"), Value::Tag(Arc::from("replace")))],
        ))
        .expect("replacement encoding should succeed");
        assert_eq!(
            replacement,
            Value::IntList(Arc::new(vec![
                0x61, 0x26, 0x23, 0x31, 0x32, 0x39, 0x34, 0x30, 0x38, 0x3b
            ]))
        );
    }

    #[test]
    fn encoding_options_require_named_tags() {
        let error = decode(BuiltinFnArgs::with_named(
            smallvec![Value::Int(65), "utf-8".into_wq_value()],
            vec![(Arc::from("mode"), "replace".into_wq_value())],
        ))
        .expect_err("a string mode should be rejected");
        assert_eq!(error.err_type, WqErrorType::Domain);
        assert_eq!(
            error.notes.first().map(String::as_str),
            Some("at named argument 'mode'")
        );

        let positional = decode(BuiltinFnArgs::from(smallvec![
            Value::Int(65),
            "utf-8".into_wq_value(),
            Value::Tag(Arc::from("replace")),
        ]))
        .expect_err("a positional mode should be rejected");
        assert_eq!(positional.err_type, WqErrorType::Arity);
    }
}
