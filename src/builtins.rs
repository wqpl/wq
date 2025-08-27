use crate::value::{Value, WqError, WqResult};
use once_cell::sync::Lazy;
use std::collections::HashMap;

use std::sync::Mutex;

mod core;
mod dict;
pub mod io;
mod list;
mod logical;
mod math;
mod str;
mod type_builtins;
mod viz;

static IOTA_CACHE: Lazy<Mutex<HashMap<i64, Value>>> = Lazy::new(|| Mutex::new(HashMap::new()));

fn arity_error(func: &str, expected: &str, got: usize) -> WqError {
    WqError::ArityError(format!("{func} expects {expected}, got {got}"))
}

/// builtin functions
pub type BuiltinFn = fn(&[Value]) -> WqResult<Value>;
pub struct Builtins {
    functions: Vec<BuiltinFn>,
    name_to_id: HashMap<String, usize>,
}

impl Default for Builtins {
    fn default() -> Self {
        Self::new()
    }
}

impl Builtins {
    pub fn new() -> Self {
        let mut builtins = Builtins {
            functions: Vec::new(),
            name_to_id: HashMap::new(),
        };
        builtins.register_functions();
        builtins
    }

    fn add(&mut self, name: &str, func: fn(&[Value]) -> WqResult<Value>) {
        let id = self.functions.len();
        self.functions.push(func);
        self.name_to_id.insert(name.to_string(), id);
    }

    fn register_functions(&mut self) {
        // Core
        self.add("match?", core::wq_match);
        self.add("hash", core::hash);
        self.add("chr", core::chr);
        self.add("ord", core::ord);
        self.add("echo", core::echo);
        #[cfg(not(target_arch = "wasm32"))]
        self.add("exec", core::exec);
        self.add("input", core::input);

        // Arithmetic
        self.add("abs", math::abs);
        self.add("neg", math::neg);
        self.add("signum", math::signum);
        self.add("sqrt", math::sqrt);
        self.add("exp", math::exp);
        self.add("ln", math::ln);
        self.add("floor", math::floor);
        self.add("ceil", math::ceil);
        self.add("rand", math::rand);
        self.add("sin", math::sin);
        self.add("cos", math::cos);
        self.add("tan", math::tan);
        self.add("arcsin", math::arcsin);
        self.add("arccos", math::arccos);
        self.add("arctan", math::arctan);
        self.add("sinh", math::sinh);
        self.add("cosh", math::cosh);
        self.add("tanh", math::tanh);
        self.add("hex", math::hex);
        self.add("bin", math::bin);

        // List
        self.add("iota", list::iota);
        self.add("range", list::range);
        self.add("count", list::count);
        self.add("first", list::first);
        self.add("last", list::last);
        self.add("reverse", list::reverse);
        self.add("sum", list::sum);
        self.add("max", list::max);
        self.add("min", list::min);
        self.add("avg", list::avg);
        self.add("take", list::take);
        self.add("drop", list::drop);
        self.add("where", list::wq_where);
        self.add("uniq", list::uniq);
        self.add("sort", list::sort);
        self.add("cat", list::cat);
        self.add("flatten", list::flatten);
        self.add("shape", list::shape);
        self.add("alloc", list::alloc);
        self.add("idx", list::idx);
        self.add("find", list::find);
        self.add("in", list::wq_in);

        // Type
        self.add("type", type_builtins::type_of_simple);
        self.add("typev", type_builtins::type_of_verbose);
        self.add("symbol", type_builtins::to_symbol);
        self.add("null?", type_builtins::is_null);

        // String
        self.add("fmt", str::fmt);
        self.add("str", str::to_str);

        // Dict
        self.add("keys", dict::keys);

        // Logical & bitwise
        self.add("and", logical::and);
        self.add("or", logical::or);
        self.add("not", logical::not);
        self.add("xor", logical::xor);
        self.add("band", logical::band);
        self.add("bor", logical::bor);
        self.add("bxor", logical::bxor);
        self.add("bnot", logical::bnot);
        self.add("shl", logical::shl);
        self.add("shr", logical::shr);

        // IO
        self.add("open", io::open);
        self.add("fexists?", io::fexists);
        self.add("mkdir", io::mkdir);
        self.add("fsize", io::fsize);
        self.add("fwrite", io::fwrite);
        self.add("fwritet", io::fwritet);
        self.add("fread", io::fread);
        self.add("freadt", io::freadt);
        self.add("freadtln", io::freadtln);
        self.add("ftell", io::ftell);
        self.add("fseek", io::fseek);
        self.add("fclose", io::fclose);
        self.add("decode", io::decode);
        self.add("encode", io::encode);

        // Visualization
        self.add("showt", viz::show_table::show_table);
        self.add("asciiplot", viz::asciiplot);
    }

    pub fn call(&self, name: &str, args: &[Value]) -> WqResult<Value> {
        if let Some(id) = self.name_to_id.get(name) {
            self.call_id(*id, args)
        } else {
            Err(WqError::ValueError(format!(
                "Unknown builtin function: {name}",
            )))
        }
    }

    pub fn call_id(&self, id: usize, args: &[Value]) -> WqResult<Value> {
        if let Some(&func) = self.functions.get(id) {
            func(args)
        } else {
            Err(WqError::ValueError("invalid builtin id".into()))
        }
    }

    pub fn has_function(&self, name: &str) -> bool {
        self.name_to_id.contains_key(name)
    }

    pub fn get_id(&self, name: &str) -> Option<usize> {
        self.name_to_id.get(name).cloned()
    }

    pub fn list_functions(&self) -> Vec<String> {
        self.name_to_id.keys().cloned().collect()
    }
}

fn values_to_strings(args: &[Value]) -> WqResult<Vec<String>> {
    args.iter()
        .map(|v| match v {
            Value::List(chars) if chars.iter().all(|c| matches!(c, Value::Char(_))) => {
                let s: String = chars
                    .iter()
                    .map(|c| {
                        if let Value::Char(ch) = *c {
                            ch
                        } else {
                            unreachable!()
                        }
                    })
                    .collect();
                Ok(s)
            }
            Value::Symbol(s) => Ok(s.clone()),
            Value::Char(ch) => Ok(ch.to_string()),
            other => Err(WqError::TypeError(format!(
                "string args expected, got '{}' of type {}",
                v,
                other.type_name_verbose()
            ))),
        })
        .collect()
}

fn value_to_string(v: &Value) -> WqResult<String> {
    Ok(values_to_strings(&[v.clone()])?.pop().unwrap())
}
