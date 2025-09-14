use crate::{
    builtins::{BuiltinEnum as BE, wqerr_ext::check_arity},
    value::{IntoWqValue as _, Value, WqResult},
    vm::Vm,
    wqerr::{WqErr, WqErrType},
};

use encoding_rs::Encoding;

fn find_encoding(label: &str) -> Option<&'static Encoding> {
    Encoding::for_label(label.as_bytes())
}

pub fn decode(_vm: &mut Vm, args: &[Value]) -> WqResult<Value> {
    check_arity(BE::Decode, [2, 3], args)?;
    let (codec, mode, bytes) = match &args {
        [c, b] => {
            let codec = c.try_to_string().map_err(|e| e.src(BE::Decode).at_arg(0))?;
            let bytes = b.try_to_vec_u8().map_err(|e| e.src(BE::Decode).at_arg(1))?;
            (codec, "s".to_string(), bytes)
        }
        [c, m, b] => {
            let codec = c.try_to_string().map_err(|e| e.src(BE::Decode).at_arg(0))?;
            let mode = m.try_to_string().map_err(|e| e.src(BE::Decode).at_arg(1))?;
            let bytes = b.try_to_vec_u8().map_err(|e| e.src(BE::Decode).at_arg(2))?;
            (codec, mode, bytes)
        }
        _ => unreachable!(),
    };

    let enc = find_encoding(&codec).ok_or_else(|| {
        WqErr::new(WqErrType::Encode)
            .src(BE::Decode)
            .msg(format!("unsupported codec '{codec}'"))
    })?;
    let (text, had_errors) = enc.decode_without_bom_handling(&bytes);
    let s = match mode.as_str() {
        "s" => {
            if had_errors {
                return Err(WqErr::new(WqErrType::Encode)
                    .src(BE::Decode)
                    .msg("strict mode decode error"));
            }
            text
        }
        "r" => text,
        _ => {
            return Err(WqErr::new(WqErrType::Domain)
                .src(BE::Decode)
                .msg("expected valid decode mode")
                .attach_note("valid mode is s (strict) or r (replace)"));
        }
    };
    Ok(s.into_wq_value())
}

pub fn encode(_vm: &mut Vm, args: &[Value]) -> WqResult<Value> {
    check_arity(BE::Encode, [2, 3], args)?;
    let (codec, mode, s) = match &args {
        [c, s] => {
            let codec = c.try_to_string().map_err(|e| e.src(BE::Encode).at_arg(0))?;
            let text = s.try_to_string().map_err(|e| e.src(BE::Encode).at_arg(1))?;
            (codec, "s".to_string(), text)
        }
        [c, m, s] => {
            let codec = c.try_to_string().map_err(|e| e.src(BE::Encode).at_arg(0))?;
            let mode = m.try_to_string().map_err(|e| e.src(BE::Encode).at_arg(1))?;
            let text = s.try_to_string().map_err(|e| e.src(BE::Encode).at_arg(2))?;
            (codec, mode, text)
        }
        _ => unreachable!(),
    };
    let enc = find_encoding(&codec).ok_or_else(|| {
        WqErr::new(WqErrType::Encode)
            .src(BE::Encode)
            .msg(format!("unsupported codec '{codec}'"))
    })?;
    let out: Vec<u8> = match mode.as_str() {
        "s" => {
            let (cow, _enc_used, had_errors) = enc.encode(&s);
            if had_errors {
                return Err(WqErr::new(WqErrType::Encode)
                    .src(BE::Encode)
                    .msg("strict mode encode error"));
            }
            cow.into_owned()
        }
        "r" => {
            let (cow, _enc_used, _had_errors) = enc.encode(&s);
            cow.into_owned()
        }
        _ => {
            return Err(WqErr::new(WqErrType::Domain)
                .src(BE::Encode)
                .msg("expected valid decode mode")
                .attach_note("valid mode is s (strict) or r (replace)"));
        }
    };
    Ok(Value::IntList(out.into_iter().map(|b| b.into()).collect()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_decode_roundtrip() {
        let mut vm = Vm::new(vec![]);
        let s = "héllo".into_wq_value();
        let b = encode(&mut vm, &["utf-8".into_wq_value(), s.clone()]).unwrap();
        let d = decode(&mut vm, &["utf-8".into_wq_value(), b]).unwrap();
        assert_eq!(d, s);
    }
}
