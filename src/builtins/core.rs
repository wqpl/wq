use std::hash::{DefaultHasher, Hash, Hasher};

use crate::{
    builtins::arity_error,
    value::{Value, WqError, WqResult},
};

pub fn wq_match(args: &[Value]) -> WqResult<Value> {
    if args.is_empty() {
        return Ok(Value::Bool(true));
    }

    let first = &args[0];
    let all_equal = args.iter().all(|x| x == first);

    Ok(Value::Bool(all_equal))
}

trait TryHash {
    fn try_hash<H: Hasher>(&self, state: &mut H) -> WqResult<()>;
}

impl TryHash for Value {
    fn try_hash<H: Hasher>(&self, state: &mut H) -> WqResult<()> {
        match self {
            Value::Int(i) => {
                0u8.hash(state); // type tag
                i.hash(state);
                Ok(())
            }
            Value::Char(c) => {
                1u8.hash(state);
                c.hash(state);
                Ok(())
            }
            Value::Bool(b) => {
                2u8.hash(state);
                b.hash(state);
                Ok(())
            }
            Value::Symbol(s) => {
                3u8.hash(state);
                s.hash(state);
                Ok(())
            }
            Value::Float(f) => {
                // floats are hashable unless NaN; +0.0 and -0.0 hash differently
                if f.is_nan() {
                    return Err(WqError::DomainError("Cannot hash NaN".into()));
                }
                4u8.hash(state);
                f.to_bits().hash(state);
                Ok(())
            }
            Value::List(xs) => {
                5u8.hash(state);
                xs.len().hash(state);
                for x in xs {
                    // Abort on the first unhashable element
                    x.try_hash(state)?
                }
                Ok(())
            }
            Value::IntList(xs) => {
                6u8.hash(state);
                xs.hash(state);
                Ok(())
            }
            Value::Dict(map) => {
                7u8.hash(state);

                // make hashing order-independent
                let mut entry_hashes: Vec<u64> = Vec::with_capacity(map.len());
                for (k, v) in map {
                    let mut entry_hasher = DefaultHasher::new();
                    k.hash(&mut entry_hasher);
                    v.try_hash(&mut entry_hasher)?;
                    entry_hashes.push(entry_hasher.finish());
                }
                entry_hashes.sort_unstable();
                entry_hashes.hash(state);

                Ok(())
            }
            _ => Err(WqError::DomainError(format!(
                "Cannot hash '{self}' of type {}",
                self.type_name_verbose()
            ))),
        }
    }
}

pub fn hash(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("hash", "1 argument", args.len()));
    }

    let mut hasher = DefaultHasher::new();
    args[0].try_hash(&mut hasher)?;
    Ok(Value::Int(hasher.finish() as i64))
}
