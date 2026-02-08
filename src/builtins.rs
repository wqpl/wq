mod core;
mod dict;
mod encoding;
mod higher_order;
mod io;
mod list;
mod list_gen;
mod logical;
mod mat;
mod math;
mod str;
mod viz;
mod wq_type;
mod wqerror_helper;

use crate::{
    value::{Value, WqResult, bc::BcResult},
    vm::Vm,
    wqerror::{WqError, WqErrorType},
};

use ahash::AHashMap;

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum BuiltinGroup {
    Intrinsic,
    CorePure,
    CoreIo,
    Exec,
    Dict,
    HigherOrder,
    Encoding,
    FileIo,
    List,
    ListGen,
    Logical,
    Math,
    Rand,
    Mat,
    Str,
    Viz,
    Type,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum BuiltinPreset {
    All,
    Pure,
    Minimal,
    Constrained,
}

impl BuiltinPreset {
    pub fn names() -> &'static [&'static str] {
        &["all", "pure", "minimal", "constrained"]
    }

    pub fn from_name(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "all" => Some(BuiltinPreset::All),
            "pure" => Some(BuiltinPreset::Pure),
            "minimal" | "min" => Some(BuiltinPreset::Minimal),
            "constrained" | "cons" => Some(BuiltinPreset::Constrained),
            _ => None,
        }
    }
}

/// builtin functions
pub type BuiltinFn = fn(&mut Vm, &[Value]) -> WqResult<Value>;
#[derive(Clone)]
pub struct Builtins {
    functions: Vec<BuiltinFn>,
    name_to_id: AHashMap<String, usize>,
    enabled: Vec<bool>,
}

impl Default for Builtins {
    fn default() -> Self {
        Self::new()
    }
}

impl Builtins {
    pub fn new() -> Self {
        Self::with_preset(BuiltinPreset::All)
    }

    pub fn with_preset(preset: BuiltinPreset) -> Self {
        let mut builtins = Builtins {
            functions: Vec::new(),
            name_to_id: AHashMap::new(),
            enabled: Vec::new(),
        };
        builtins.register_functions();
        builtins.apply_preset(preset);
        builtins
    }

    pub fn apply_preset(&mut self, preset: BuiltinPreset) {
        let total = self.functions.len();
        debug_assert_eq!(BUILTIN_GROUPS.len(), total, "builtin group map out of sync");
        self.enabled = vec![false; total];
        for (idx, group) in BUILTIN_GROUPS.iter().enumerate() {
            let enabled = match preset {
                BuiltinPreset::All => true,
                BuiltinPreset::Minimal => *group == BuiltinGroup::Intrinsic,
                BuiltinPreset::Pure => !matches!(
                    group,
                    BuiltinGroup::CoreIo
                        | BuiltinGroup::Exec
                        | BuiltinGroup::FileIo
                        | BuiltinGroup::Viz
                        | BuiltinGroup::Rand
                ),
                BuiltinPreset::Constrained => {
                    !matches!(group, BuiltinGroup::Exec | BuiltinGroup::FileIo)
                }
            };
            if enabled {
                self.enabled[idx] = true;
            }
        }
        self.force_intrinsics();
    }

    pub fn is_enabled_id(&self, id: u16) -> bool {
        self.enabled.get(id as usize).copied().unwrap_or(false)
    }

    pub fn is_enabled_name(&self, name: &str) -> bool {
        self.name_to_id
            .get(name)
            .and_then(|id| self.enabled.get(*id))
            .copied()
            .unwrap_or(false)
    }

    pub fn is_known_name(&self, name: &str) -> bool {
        self.name_to_id.contains_key(name)
    }

    pub fn is_disabled_name(&self, name: &str) -> bool {
        self.is_known_name(name) && !self.is_enabled_name(name)
    }

    fn add(&mut self, name: &str, func: BuiltinFn) {
        let id = self.functions.len();
        self.functions.push(func);
        self.name_to_id.insert(name.to_string(), id);
    }

    pub fn has_function(&self, name: &str) -> bool {
        self.is_enabled_name(name)
    }

    pub fn get_id(&self, name: &str) -> Option<usize> {
        self.name_to_id
            .get(name)
            .copied()
            .filter(|id| self.enabled.get(*id).copied().unwrap_or(false))
    }

    pub fn get_fn_by_id(&self, id: usize) -> Option<&BuiltinFn> {
        match self.enabled.get(id) {
            Some(true) => self.functions.get(id),
            _ => None,
        }
    }

    pub fn list_functions(&self) -> Vec<String> {
        self.name_to_id
            .iter()
            .filter_map(|(name, id)| {
                if self.enabled.get(*id).copied().unwrap_or(false) {
                    Some(name.clone())
                } else {
                    None
                }
            })
            .collect()
    }

    pub fn list_functions_all(&self) -> Vec<String> {
        self.name_to_id.keys().cloned().collect()
    }

    fn force_intrinsics(&mut self) {
        for (idx, group) in BUILTIN_GROUPS.iter().enumerate() {
            if *group == BuiltinGroup::Intrinsic
                && let Some(slot) = self.enabled.get_mut(idx)
            {
                *slot = true;
            }
        }
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
    (MAP, Map, "map", "map[f;xs], map[d;f;xs]", "2, 3", higher_order::map),
    (ZIPW, ZipW, "zipw", "zipw[f;xs;ys], zipw[d;f;xs;ys]", "3, 4", higher_order::zipw),
    (FOLD, Fold, "fold", "fold[f;xs], fold[f;acc;xs]", "2, 3", higher_order::fold),
    (SCAN, Scan, "scan", "scan[f;xs], scan[f;acc;xs]", "2, 3", higher_order::scan),

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
    (SHOWTABLE, Showtable, "showtable", "showtable[table]", "1", viz::show_table::show_table),
    (ASCIIPLOT, Asciiplot, "asciiplot", "asciiplot[data+], asciiplot[data+;opts]", "1+", viz::asciiplot::asciiplot),

    // Type =========================================================
    (TYPE, Type, "type", "type[x]", "1", wq_type::type_of),
    (SYMBOL, Symbol, "symbol", "symbol[x]", "1", wq_type::to_symbol),
    (ATOM_Q, AtomQ, "atom?", "atom?[x]", "1", wq_type::is_atom),
    (UNIT_Q, UnitQ, "unit?", "unit?[x]", "1", wq_type::is_unit),
}

const BUILTIN_GROUPS: &[BuiltinGroup] = &[
    // Core =========================================================
    BuiltinGroup::CoreIo,   // print
    BuiltinGroup::CoreIo,   // echo
    BuiltinGroup::CoreIo,   // input
    BuiltinGroup::CorePure, // bfn
    BuiltinGroup::CorePure, // chr
    BuiltinGroup::CorePure, // ord
    BuiltinGroup::CorePure, // int
    BuiltinGroup::CorePure, // bin
    BuiltinGroup::CorePure, // oct
    BuiltinGroup::CorePure, // hex
    BuiltinGroup::CorePure, // raise
    #[cfg(not(target_arch = "wasm32"))]
    BuiltinGroup::Exec, // exec
    // Dict =========================================================
    BuiltinGroup::Dict, // keys
    BuiltinGroup::Dict, // haskey?
    BuiltinGroup::Dict, // itk
    BuiltinGroup::Dict, // kti
    // Higher-order =========================================================
    BuiltinGroup::HigherOrder, // map
    BuiltinGroup::HigherOrder, // zipw
    BuiltinGroup::HigherOrder, // fold
    BuiltinGroup::HigherOrder, // scan
    // IO (encoding) =========================================================
    BuiltinGroup::Encoding, // decode
    BuiltinGroup::Encoding, // encode
    #[cfg(not(target_arch = "wasm32"))]
    BuiltinGroup::FileIo, // open
    #[cfg(not(target_arch = "wasm32"))]
    BuiltinGroup::FileIo, // fexists?
    #[cfg(not(target_arch = "wasm32"))]
    BuiltinGroup::FileIo, // mkdir
    #[cfg(not(target_arch = "wasm32"))]
    BuiltinGroup::FileIo, // fsize
    #[cfg(not(target_arch = "wasm32"))]
    BuiltinGroup::FileIo, // fwrite
    #[cfg(not(target_arch = "wasm32"))]
    BuiltinGroup::FileIo, // fwritet
    #[cfg(not(target_arch = "wasm32"))]
    BuiltinGroup::FileIo, // fread
    #[cfg(not(target_arch = "wasm32"))]
    BuiltinGroup::FileIo, // freadt
    #[cfg(not(target_arch = "wasm32"))]
    BuiltinGroup::FileIo, // freadtln
    #[cfg(not(target_arch = "wasm32"))]
    BuiltinGroup::FileIo, // fseek
    #[cfg(not(target_arch = "wasm32"))]
    BuiltinGroup::FileIo, // ftell
    #[cfg(not(target_arch = "wasm32"))]
    BuiltinGroup::FileIo, // fclose
    // List =========================================================
    BuiltinGroup::Intrinsic, // len
    BuiltinGroup::List,      // shape
    BuiltinGroup::List,      // depth
    BuiltinGroup::List,      // uniform?
    BuiltinGroup::List,      // sum
    BuiltinGroup::List,      // min
    BuiltinGroup::List,      // max
    BuiltinGroup::List,      // flatten
    BuiltinGroup::List,      // reverse
    BuiltinGroup::List,      // sort
    BuiltinGroup::List,      // filter
    BuiltinGroup::List,      // find
    BuiltinGroup::ListGen,   // alloc
    BuiltinGroup::ListGen,   // till
    BuiltinGroup::ListGen,   // iota
    BuiltinGroup::ListGen,   // reshape
    BuiltinGroup::ListGen,   // where
    // Logical ======================================================
    BuiltinGroup::Logical, // not
    BuiltinGroup::Logical, // and
    BuiltinGroup::Logical, // or
    BuiltinGroup::Logical, // xor
    BuiltinGroup::Logical, // bnot
    BuiltinGroup::Logical, // band
    BuiltinGroup::Logical, // bor
    BuiltinGroup::Logical, // bxor
    BuiltinGroup::Logical, // shl
    BuiltinGroup::Logical, // shr
    // Math =========================================================
    BuiltinGroup::Math, // neg
    BuiltinGroup::Math, // abs
    BuiltinGroup::Math, // sgn
    BuiltinGroup::Math, // sqrt
    BuiltinGroup::Math, // exp
    BuiltinGroup::Math, // ln
    BuiltinGroup::Math, // log2
    BuiltinGroup::Math, // log10
    BuiltinGroup::Math, // floor
    BuiltinGroup::Math, // ceil
    BuiltinGroup::Math, // round
    BuiltinGroup::Math, // sin
    BuiltinGroup::Math, // cos
    BuiltinGroup::Math, // tan
    BuiltinGroup::Math, // arcsin
    BuiltinGroup::Math, // arccos
    BuiltinGroup::Math, // arctan
    BuiltinGroup::Math, // sinh
    BuiltinGroup::Math, // cosh
    BuiltinGroup::Math, // tanh
    BuiltinGroup::Math, // arcsinh
    BuiltinGroup::Math, // arccosh
    BuiltinGroup::Math, // arctanh
    BuiltinGroup::Math, // log
    BuiltinGroup::Math, // arctan2
    BuiltinGroup::Rand, // rand
    // Matrix =========================================================
    BuiltinGroup::Mat, // transpose
    // Str =========================================================
    BuiltinGroup::Str,       // str
    BuiltinGroup::Intrinsic, // fmt
    BuiltinGroup::Str,       // graphemes
    BuiltinGroup::Str,       // ws?
    BuiltinGroup::Str,       // words
    BuiltinGroup::Str,       // trim
    BuiltinGroup::Str,       // trims
    BuiltinGroup::Str,       // trime
    // Visualization =========================================================
    BuiltinGroup::Viz, // showt
    BuiltinGroup::Viz, // asciiplot
    // Type =========================================================
    BuiltinGroup::Type, // type
    BuiltinGroup::Type, // symbol
    BuiltinGroup::Type, // atom?
    BuiltinGroup::Type, // unit?
];

fn fold_value<F>(src: BuiltinEnum, args: &[Value], f: F) -> WqResult<Value>
where
    F: Fn(&Value, &Value) -> BcResult<Value>,
{
    if args.len() < 2 {
        return Err(WqError::new(WqErrorType::Arity)
            .msg(format!("expected 2 or more args, got {}", args.len())));
    }
    let mut acc = args[0].clone();
    for v in &args[1..] {
        acc = f(&acc, v).map_err(|e| e.into_wqerror().src(src))?;
    }
    Ok(acc)
}
