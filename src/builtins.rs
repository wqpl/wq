use crate::value::{Value, WqResult};
use crate::wqerror::WqError;
use once_cell::sync::Lazy;
use std::collections::HashMap;

use std::sync::Mutex;

mod core;
mod dict;
pub mod io;
pub mod list;
mod logical;
mod math;
mod str;
mod viz;
mod wq_type;

static INTS_CACHE: Lazy<Mutex<HashMap<i64, Value>>> = Lazy::new(|| Mutex::new(HashMap::new()));

fn arity_error(func: &str, expected: &str, got: usize) -> WqError {
    WqError::ArityError(format!("`{func}`: expected {expected} arg(s), got {got}"))
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
        self.add("m?", core::wq_match);
        self.add("hash", core::hash);
        self.add("chr", core::chr);
        self.add("hex", core::hex);
        self.add("bin", core::bin);
        self.add("oct", core::oct);
        self.add("int", core::int);
        self.add("ord", core::ord);
        self.add("print", core::print);
        self.add("println", core::println);
        // alias
        self.add("echo", core::println);
        self.add("input", core::input);
        self.add("raise", core::raise);
        #[cfg(not(target_arch = "wasm32"))]
        self.add("exec", core::exec);

        // type
        self.add("type", wq_type::wq_type_of);
        self.add("typev", wq_type::wq_typev);
        self.add("symbol", wq_type::to_symbol);
        // self.add("null?", wq_type::is_null);
        self.add("atom?", wq_type::is_atom);
        self.add("int?", wq_type::is_int);
        self.add("number?", wq_type::is_number);
        self.add("fn?", wq_type::is_fn);
        self.add("stream?", wq_type::is_stream);
        self.add("list?", wq_type::is_list);
        self.add("str?", wq_type::is_str);
        self.add("unit?", wq_type::is_unit);
        self.add("dict?", wq_type::is_dict);

        // Arithmetic
        self.add("abs", math::abs);
        self.add("neg", math::neg);
        self.add("sgn", math::sgn);
        self.add("sqrt", math::sqrt);
        self.add("exp", math::exp);
        self.add("ln", math::ln);
        self.add("log2", math::log2);
        self.add("log10", math::log10);
        self.add("log", math::log);
        self.add("floor", math::floor);
        self.add("ceil", math::ceil);
        self.add("round", math::round);
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
        self.add("arcsinh", math::arcsinh);
        self.add("arccosh", math::arccosh);
        self.add("arctanh", math::arctanh);
        // self.add("hex", math::hex);
        // self.add("bin", math::bin);

        // List
        self.add("cat", list::cat);
        self.add("count", list::count);
        self.add("alloc", list::alloc);
        self.add("iota", list::iota);
        self.add("ints", list::ints);
        self.add("reshape", list::reshape);
        self.add("shape", list::shape);
        self.add("depth", list::depth);
        self.add("intlist?", list::is_intlist);
        self.add("uniform?", list::is_uniform);
        self.add("rg", list::range);
        self.add("reverse", list::reverse);
        self.add("where", list::wq_where);
        self.add("flatten", list::flatten);
        self.add("sort", list::sort);
        // self.add("fst", list::fst);
        // self.add("lst", list::lst);
        // self.add("sum", list::sum);
        // self.add("max", list::max);
        // self.add("min", list::min);
        // self.add("avg", list::avg);
        // self.add("take", list::take);
        // self.add("drop", list::drop);
        // self.add("distinct", list::distinct);
        // self.add("sort", list::sort);
        // self.add("idx", list::idx);
        // self.add("find", list::find);
        // self.add("in", list::wq_in);

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
        #[cfg(not(target_arch = "wasm32"))]
        {
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
        }
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
