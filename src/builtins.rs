mod core;
mod dict;
mod encoding;
mod ho;
mod io;
mod list;
mod list_gen;
mod logical;
mod mat;
mod math;
mod str;
mod viz;
mod wq_type;
mod wqerr_ext;

use crate::{
    value::{Value, WqResult, bc::BcResult},
    vm::Vm,
    wqerr::{WqErr, WqErrType},
};

use ahash::AHashMap;

/// builtin functions
pub type BuiltinFn = fn(&mut Vm, &[Value]) -> WqResult<Value>;
pub struct Builtins {
    functions: Vec<BuiltinFn>,
    name_to_id: AHashMap<String, usize>,
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
            name_to_id: AHashMap::new(),
        };
        builtins.register_functions();
        builtins
    }

    fn add(&mut self, name: &str, func: BuiltinFn) {
        let id = self.functions.len();
        self.functions.push(func);
        self.name_to_id.insert(name.to_string(), id);
    }

    pub fn has_function(&self, name: &str) -> bool {
        self.name_to_id.contains_key(name)
    }

    pub fn get_id(&self, name: &str) -> Option<usize> {
        self.name_to_id.get(name).cloned()
    }

    pub fn get_fn_by_id(&self, id: usize) -> Option<&BuiltinFn> {
        self.functions.get(id)
    }

    pub fn list_functions(&self) -> Vec<String> {
        self.name_to_id.keys().cloned().collect()
    }
}

// Entry
macro_rules! declare_builtins {
    ( $($tt:tt)* ) => {
        __decl_builtins_flat! { () $($tt)* , } // ensure a trailing comma
    };
}

// Flatten top-level stream into a uniform list of items
macro_rules! __decl_builtins_flat {
    // Grouped attrs: #[attr]+ { ... }, then a comma and rest
    ( ($($acc:tt)*) $(#[$attr:meta])+ { $($group:tt)* } , $($rest:tt)* ) => {
        __decl_builtins_group! { ( $($acc)* ) ( $(#[$attr])* ) { $($group)* } $($rest)* }
    };

    // Single item with attrs
    ( ($($acc:tt)*) $(#[$attr:meta])+ ($($one:tt)*) , $($rest:tt)* ) => {
        __decl_builtins_flat! { ( $($acc)* $(#[$attr])* ($($one)*), ) $($rest)* }
    };

    // Plain single item
    ( ($($acc:tt)*) ($($one:tt)*) , $($rest:tt)* ) => {
        __decl_builtins_flat! { ( $($acc)* ($($one)*), ) $($rest)* }
    };

    // Skip stray commas
    ( ($($acc:tt)*) , $($rest:tt)* ) => {
        __decl_builtins_flat! { ( $($acc)* ) $($rest)* }
    };

    // Done: forward to your original implementation macro
    ( ($($flat:tt)*) ) => {
        __declare_builtins_impl! { $($flat)* }
    };
}

// Eat a group one item at a time, reusing the same outer attrs each step
macro_rules! __decl_builtins_group {
    // Head item + tail (note the comma inside the braces)
    ( ($($acc:tt)*) ($($outer:tt)*) { $(#[$inner:meta])* ($($one:tt)*) , $($tail:tt)* } $($rest:tt)* ) => {
        __decl_builtins_group! {
            ( $($acc)* $($outer)* $(#[$inner])* ($($one)*), )
            ($($outer)*)
            { $($tail)* }
            $($rest)*
        }
    };

    // Last item in the group (no trailing comma inside the braces)
    ( ($($acc:tt)*) ($($outer:tt)*) { $(#[$inner:meta])* ($($one:tt)*) } $($rest:tt)* ) => {
        __decl_builtins_flat! {
            ( $($acc)* $($outer)* $(#[$inner])* ($($one)*), )
            $($rest)*
        }
    };

    // Empty group
    ( ($($acc:tt)*) ($($outer:tt)*) { } $($rest:tt)* ) => {
        __decl_builtins_flat! { ( $($acc)* ) $($rest)* }
    };
}

macro_rules! __declare_builtins_impl {
    ($($(#[$m:meta])? ($CONST:ident, $VAR:ident, $name:expr, $usage:expr, $arity:tt, $func:path),)+) => {
        #[repr(u16)]
        #[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
        pub enum BuiltinEnum { $( $(#[$m])? $VAR ),+ }

        impl BuiltinEnum {
            pub const fn id(self) -> u16 { self as u16 }

            pub const fn name(self) -> &'static str {
                match self { $($(#[$m])? BuiltinEnum::$VAR => $name,)+ }
            }
            pub const fn usage(self) -> &'static str {
                match self { $($(#[$m])? BuiltinEnum::$VAR => $usage,)+ }
            }
            pub const fn arity(self) -> &'static str {
                match self { $($(#[$m])? BuiltinEnum::$VAR => $arity,)+ }
            }
        }

        impl Builtins {
            $($(#[$m])? pub const $CONST: u16 = BuiltinEnum::$VAR as u16;)+

            pub const NAMES: &'static [&'static str] = &[$($(#[$m])? $name ),+];
            pub const USAGES: &'static [&'static str] = &[$($(#[$m])? $usage ),+];
            pub const ARITIES: &'static [&'static str] = &[$($(#[$m])? $arity ),+];

            #[inline]
            pub fn name_from_id(id: u16) -> Option<&'static str> {
                Self::NAMES.get(id as usize).copied()
            }

            #[inline]
            pub fn usage_from_id(id: u16) -> Option<&'static str> {
                Self::USAGES.get(id as usize).copied()
            }

            #[inline]
            pub fn arity_from_id(id: u16) -> Option<&'static str> {
                Self::ARITIES.get(id as usize).copied()
            }

            fn register_functions(&mut self) {
                $($(#[$m])? self.add($name, $func);)+
            }
        }
    };
}

declare_builtins! {
    // Core =========================================================
    (PRINT, Print, "print", "print[value*]", "0+", core::print),
    (ECHO, Echo, "echo", "echo[value*]", "0+", core::echo),
    (INPUT, Input, "input", "input[prompt?]", "0, 1", core::input),
    (BFN, Bfn, "bfn", "bfn[]", "0", core::bfn),
    (CHR, Chr, "chr", "chr[xs]", "1", core::chr),
    (ORD, Ord, "ord", "ord[xs]", "1", core::ord),
    (INT, Int, "int", "int[x], int[x;base]", "1, 2", core::int),
    (BIN, Bin, "bin", "bin[prefix?;xs]", "1, 2", core::bin),
    (OCT, Oct, "oct", "oct[prefix?;xs]", "1, 2", core::oct),
    (HEX, Hex, "hex", "hex[prefix?;xs]", "1, 2", core::hex),
    (RAISE, Raise, "raise", "raise[msg]", "1", core::raise),
    #[cfg(not(target_arch = "wasm32"))]
    (EXEC, Exec, "exec", "exec[parts+]", "1+", core::exec),

    // Dict =========================================================
    (KEYS, Keys, "keys", "keys[xs]", "1", dict::keys),
    (HAS_KEY_Q, HasKeyQ, "haskey?", "haskey?[k;xs]", "2", dict::has_key),
    (IDX_TO_KEY, IdxToKey, "itk", "itk[i;xs]", "2", dict::idx_to_key),
    (KEY_TO_IDX, KeyToIdx, "kti", "kti[k;xs]", "2", dict::key_to_idx),


    // Higher-order =========================================================
    (MAP, Map, "map", "map[f;xs], map[d;f;xs]", "2, 3", ho::map),
    (ZIPW, ZipW, "zipw", "zipw[f;xs;ys], zipw[d;f;xs;ys]", "3, 4", ho::zipw),
    (FOLD, Fold, "fold", "fold[f;xs], fold[f;acc;xs]", "2, 3", ho::fold),
    (SCAN, Scan, "scan", "scan[f;xs], scan[f;acc;xs]", "2, 3", ho::scan),

    // IO =========================================================
    (DECODE, Decode, "decode", "decode[codec;bytes], decode[codec;mode;bytes]", "2, 3", encoding::decode),
    (ENCODE, Encode, "encode", "encode[codec;text], encode[codec;mode;text]", "2, 3", encoding::encode),

    #[cfg(not(target_arch = "wasm32"))]
    {
        (OPEN, Open, "open", "open[path;flag?]", "1, 2", io::open),
        (FEXISTS_Q, FexistsQ, "fexists?", "fexists?[path]", "1", io::fexists),
        (MKDIR, Mkdir, "mkdir", "mkdir[path]", "1", io::mkdir),
        (FSIZE, Fsize, "fsize", "fsize[path]", "1", io::fsize),
        (FWRITE, Fwrite, "fwrite", "fwrite[stream;bytes]", "2", io::fwrite),
        (FWRITET, Fwritet, "fwritet", "fwritet[stream;text]", "2", io::fwritet),
        (FREAD, Fread, "fread", "fread[stream;len?]", "1, 2", io::fread),
        (FREADT, Freadt, "freadt", "freadt[stream;len]", "1, 2", io::freadt),
        (FREADTLN, Freadtln, "freadtln", "freadtln[stream]", "1", io::freadtln),
        (FSEEK, Fseek, "fseek", "fseek[stream;offset;whence?]", "2, 3", io::fseek),
        (FTELL, Ftell, "ftell", "ftell[stream]", "1", io::ftell),
        (FCLOSE, Fclose, "fclose", "fclose[stream]", "1", io::fclose),
    },

    // List =========================================================
    // (CAT, Cat, "cat", "cat[xs*]", "0+", list::cat),
    (LEN, Len, "len", "len[xs]", "1", list::len),
    (SHAPE, Shape, "shape", "shape[xs]", "1", list::shape),
    (DEPTH, Depth, "depth", "depth[xs]", "1", list::depth),
    (UNIFORM_Q, UniformQ, "uniform?", "uniform?[xs]", "1", list::is_uniform),

    (SUM, Sum, "sum", "sum[xs*]", "0+", list::sum),
    (MIN, Min, "min", "min[xs], min[xs;ys+]", "1+", list::min),
    (MAX, Max, "max", "max[xs], max[xs;ys+]", "1+", list::max),
    (FLATTEN, Flatten, "flatten", "flatten[xs]", "1", list::flatten),
    (REVERSE, Reverse, "reverse", "reverse[xs]", "1", list::reverse),
    (SORT, Sort, "sort", "sort[xs]", "1", list::sort),
    (FILTER, Filter, "filter", "filter[f;xs]", "2", list::filter),
    (FIND, Find, "find", "find[elem;xs], find[elem;threshold;xs], find[elem;threshold;depth;xs]", "2, 3, 4", list::find),

    (ALLOC, Alloc, "alloc", "alloc[shape]", "1", list_gen::alloc),
    (TILL, Till, "till", "till[shape]", "1", list_gen::till),
    (IOTA, Iota, "iota", "iota[shape]", "1", list_gen::iota),
    (RESHAPE, Reshape, "reshape", "reshape[shape;x]", "2", list_gen::reshape),
    (WHERE, Where, "where", "where[xs]", "1", list_gen::wq_where),

    // Logical ======================================================
    (NOT, Not, "not", "not[xs]", "1", logical::not),
    (AND, And, "and", "and[xs;ys+]", "2+", logical::and),
    (OR, Or, "or", "or[xs;ys+]", "2+", logical::or),
    (XOR, Xor, "xor", "xor[xs;ys+]", "2+", logical::xor),

    (BNOT, Bnot, "bnot", "bnot[xs]", "1", logical::bnot),
    (BAND, Band, "band", "band[xs;ys+]", "2+", logical::band),
    (BOR, Bor, "bor", "bor[xs;ys+]", "2+", logical::bor),
    (BXOR, Bxor, "bxor", "bxor[xs;ys+]", "2+", logical::bxor),

    (SHL, Shl, "shl", "shl[xs;shift]", "2", logical::shl),
    (SHR, Shr, "shr", "shr[xs;shift]", "2", logical::shr),

    // Math =========================================================
    (NEG, Neg, "neg", "neg[xs]", "1", math::neg),
    (ABS, Abs, "abs", "abs[xs]", "1", math::abs),
    (SGN, Sgn, "sgn", "sgn[xs]", "1", math::sgn),
    (SQRT, Sqrt, "sqrt", "sqrt[xs]", "1", math::sqrt),
    (EXP, Exp, "exp", "exp[xs]", "1", math::exp),
    (LN, Ln, "ln", "ln[xs]", "1", math::ln),
    (LOG2, Log2, "log2", "log2[xs]", "1", math::log2),
    (LOG10, Log10, "log10", "log10[xs]", "1", math::log10),
    (FLOOR, Floor, "floor", "floor[xs]", "1", math::floor),
    (CEIL, Ceil, "ceil", "ceil[xs]", "1", math::ceil),
    (ROUND, Round, "round", "round[xs]", "1", math::round),
    (SIN, Sin, "sin", "sin[xs]", "1", math::sin),
    (COS, Cos, "cos", "cos[xs]", "1", math::cos),
    (TAN, Tan, "tan", "tan[xs]", "1", math::tan),
    (ARCSIN, Arcsin, "arcsin", "arcsin[xs]", "1", math::arcsin),
    (ARCCOS, Arccos, "arccos", "arccos[xs]", "1", math::arccos),
    (ARCTAN, Arctan, "arctan", "arctan[xs]", "1", math::arctan),
    (SINH, Sinh, "sinh", "sinh[xs]", "1", math::sinh),
    (COSH, Cosh, "cosh", "cosh[xs]", "1", math::cosh),
    (TANH, Tanh, "tanh", "tanh[xs]", "1", math::tanh),
    (ARCSINH, Arcsinh, "arcsinh", "arcsinh[xs]", "1", math::arcsinh),
    (ARCCOSH, Arccosh, "arccosh", "arccosh[xs]", "1", math::arccosh),
    (ARCTANH, Arctanh, "arctanh", "arctanh[xs]", "1", math::arctanh),

    (LOG, Log, "log", "log[x;a]", "2", math::log),
    (ARCTAN2, Arctan2, "arctan2", "arctan2[xs]", "1", math::arctan2),

    (RAND, Rand, "rand", "rand[], rand[upper], rand[lower;upper]", "0, 1, 2", math::rand),

    // Matrix =========================================================
    (TRANSPOSE, Transpose, "transpose", "transpose[x]", "1", mat::transpose),

    // Str =========================================================
    (STR, Str, "str", "str[x]", "1", str::to_str),
    (FMT, Fmt, "fmt", "fmt[template;v*]", "1+", str::fmt),
    (GRAPHEMES, Graphemes, "graphemes", "graphemes[s]", "1", str::graphemes),
    (WS_Q, WsQ, "ws?", "ws?[x]", "1", str::is_whitespace),
    (WORDS, Words, "words", "words[s]", "1", str::words),
    (TRIM, Trim, "trim", "trim[s]", "1", str::trim),
    (TRIM_S, TrimS, "trims", "trims[s]", "1", str::trim_start),
    (TRIM_E, TrimE, "trime", "trime[s]", "1", str::trim_end),

    // Visualization =========================================================
    (SHOWT, Showt, "showt", "showt[table]", "1", viz::show_table::show_table),
    (ASCIIPLOT, Asciiplot, "asciiplot", "asciiplot[data+], asciiplot[data+;opts]", "1+", viz::asciiplot::asciiplot),

    // Type =========================================================
    (TYPE, Type, "type", "type[x]", "1", wq_type::type_of),
    (SYMBOL, Symbol, "symbol", "symbol[x]", "1", wq_type::to_symbol),
    (ATOM_Q, AtomQ, "atom?", "atom?[x]", "1", wq_type::is_atom),
    (UNIT_Q, UnitQ, "unit?", "unit?[x]", "1", wq_type::is_unit),
}

fn fold_value<F>(src: BuiltinEnum, args: &[Value], f: F) -> WqResult<Value>
where
    F: Fn(&Value, &Value) -> BcResult<Value>,
{
    if args.len() < 2 {
        return Err(WqErr::new(WqErrType::Arity)
            .msg(format!("expected 2 or more args, got {}", args.len())));
    }
    let mut acc = args[0].clone();
    for v in &args[1..] {
        acc = f(&acc, v).map_err(|e| e.into_wqerror().src(src))?;
    }
    Ok(acc)
}
