mod cas;
mod complex;
mod core;
mod dict;
mod encoding;
mod fraction;
mod ho;
mod io;
mod list;
mod listgen;
mod logical;
mod math;
mod meta;
mod op;
mod set;
mod string;
mod viz;
mod wqtype;

use std::ops::{Deref, DerefMut};
use std::sync::Arc;

use ahash::AHashMap;
use smallvec::SmallVec;

use crate::interpret::vanilla::Sv4;
use crate::value::{Value, WqResult};
use crate::wqerror::{WqError, WqErrorType};

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub enum BuiltinGroup {
    Intrinsic,

    Cas,
    Complex,

    CorePure,
    CoreIO,
    Exec,

    Dict,
    Encoding,
    Fraction,
    HigherOrder,
    FileIO,
    List,
    ListGen,
    Logical,

    Math,
    Meta,
    Rand,

    Mat,
    Set,
    Str,
    Viz,
    Type,
}

impl BuiltinGroup {
    pub fn name(self) -> &'static str {
        match self {
            BuiltinGroup::Intrinsic => "Intrinsic",

            BuiltinGroup::CorePure => "Core Pure",
            BuiltinGroup::CoreIO => "Core IO",
            BuiltinGroup::Exec => "Exec",

            BuiltinGroup::Cas => "CAS",
            BuiltinGroup::Complex => "Complex",
            BuiltinGroup::Dict => "Dict",
            BuiltinGroup::Encoding => "Encoding",
            BuiltinGroup::Fraction => "Fraction",
            BuiltinGroup::HigherOrder => "Higher-Order",
            BuiltinGroup::FileIO => "File IO",
            BuiltinGroup::List => "List",
            BuiltinGroup::ListGen => "List Generation",
            BuiltinGroup::Logical => "Logical",
            BuiltinGroup::Mat => "Matrix",

            BuiltinGroup::Meta => "Meta",
            BuiltinGroup::Math => "Math",
            BuiltinGroup::Rand => "Random",

            BuiltinGroup::Set => "Set",
            BuiltinGroup::Str => "String",
            BuiltinGroup::Viz => "Visualization",
            BuiltinGroup::Type => "Type",
        }
    }

    pub(crate) fn is_pure(self) -> bool {
        #![deny(clippy::wildcard_enum_match_arm)]
        match self {
            BuiltinGroup::CoreIO
            | BuiltinGroup::Exec
            | BuiltinGroup::FileIO
            | BuiltinGroup::Viz
            | BuiltinGroup::Rand => false,

            BuiltinGroup::Intrinsic
            | BuiltinGroup::Cas
            | BuiltinGroup::Complex
            | BuiltinGroup::CorePure
            | BuiltinGroup::Dict
            | BuiltinGroup::Encoding
            | BuiltinGroup::Fraction
            | BuiltinGroup::HigherOrder
            | BuiltinGroup::List
            | BuiltinGroup::ListGen
            | BuiltinGroup::Logical
            | BuiltinGroup::Math
            | BuiltinGroup::Meta
            | BuiltinGroup::Mat
            | BuiltinGroup::Set
            | BuiltinGroup::Str
            | BuiltinGroup::Type => true,
        }
    }

    pub(crate) fn is_allowed_in_constrained_mode(self) -> bool {
        #![deny(clippy::wildcard_enum_match_arm)]
        match self {
            BuiltinGroup::Exec | BuiltinGroup::FileIO => false,

            BuiltinGroup::Intrinsic
            | BuiltinGroup::Cas
            | BuiltinGroup::Complex
            | BuiltinGroup::CorePure
            | BuiltinGroup::CoreIO
            | BuiltinGroup::Dict
            | BuiltinGroup::Encoding
            | BuiltinGroup::Fraction
            | BuiltinGroup::HigherOrder
            | BuiltinGroup::List
            | BuiltinGroup::ListGen
            | BuiltinGroup::Logical
            | BuiltinGroup::Math
            | BuiltinGroup::Meta
            | BuiltinGroup::Rand
            | BuiltinGroup::Mat
            | BuiltinGroup::Set
            | BuiltinGroup::Str
            | BuiltinGroup::Viz
            | BuiltinGroup::Type => true,
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum BuiltinPreset {
    All,
    Pure,
    Minimal,
    Constrained,
}

impl BuiltinPreset {
    pub const DEFAULT: Self = Self::All;

    pub fn names() -> &'static [&'static str] {
        &["all", "pure", "minimal", "constrained"]
    }

    pub fn name(self) -> &'static str {
        match self {
            BuiltinPreset::All => "all",
            BuiltinPreset::Pure => "pure",
            BuiltinPreset::Minimal => "minimal",
            BuiltinPreset::Constrained => "constrained",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "all" | "a" => Some(BuiltinPreset::All),
            "pure" | "p" => Some(BuiltinPreset::Pure),
            "minimal" | "m" => Some(BuiltinPreset::Minimal),
            "constrained" | "c" => Some(BuiltinPreset::Constrained),
            _ => None,
        }
    }
}

/// Owned positional and named arguments passed to a builtin.
///
/// Positional args: `SmallVec<[Value; 4]>` — zero heap for ≤4 args.
/// Named args: `Option<Vec<(Arc<str>, Value)>>` — heap-allocated, None when
/// empty.
pub struct BuiltinFnArgs {
    pos: Sv4,
    named: Option<Vec<(Arc<str>, Value)>>,
    runtime_validated: bool,
}

impl BuiltinFnArgs {
    pub fn new() -> Self {
        Self {
            pos: SmallVec::new(),
            named: None,
            runtime_validated: false,
        }
    }

    pub fn with_named(pos: Sv4, named: Vec<(Arc<str>, Value)>) -> Self {
        let named = if named.is_empty() { None } else { Some(named) };
        Self {
            pos,
            named,
            runtime_validated: false,
        }
    }

    pub fn len(&self) -> usize {
        self.pos.len()
    }
    pub fn is_empty(&self) -> bool {
        self.pos.is_empty()
    }
    pub fn has_named(&self) -> bool {
        self.named.is_some()
    }
    pub fn push(&mut self, v: Value) {
        self.runtime_validated = false;
        self.pos.push(v)
    }

    #[inline]
    pub(crate) fn mark_runtime_validated(&mut self) {
        self.runtime_validated = true;
    }

    #[inline]
    fn runtime_validated(&self) -> bool {
        self.runtime_validated
    }

    /// Get the positional argument at index `n`, or `None` if out of bounds.
    pub fn get_pos(&self, n: usize) -> Option<&Value> {
        self.pos.get(n)
    }

    /// Look up a named argument by name.
    pub fn named(&self, name: &str) -> Option<&Value> {
        self.named.as_ref().and_then(|args| {
            args.iter()
                .find(|(n, _)| n.as_ref() == name)
                .map(|(_, v)| v)
        })
    }
}

impl Default for BuiltinFnArgs {
    fn default() -> Self {
        Self::new()
    }
}

impl Deref for BuiltinFnArgs {
    type Target = [Value];
    fn deref(&self) -> &[Value] {
        &self.pos
    }
}

impl DerefMut for BuiltinFnArgs {
    fn deref_mut(&mut self) -> &mut [Value] {
        &mut self.pos
    }
}

impl IntoIterator for BuiltinFnArgs {
    type Item = Value;
    type IntoIter = smallvec::IntoIter<[Value; 4]>;
    fn into_iter(self) -> Self::IntoIter {
        self.pos.into_iter()
    }
}

impl From<Sv4> for BuiltinFnArgs {
    fn from(v: Sv4) -> Self {
        Self {
            pos: v,
            named: None,
            runtime_validated: false,
        }
    }
}

impl From<Value> for BuiltinFnArgs {
    fn from(v: Value) -> Self {
        let mut pos = SmallVec::new();
        pos.push(v);
        Self {
            pos,
            named: None,
            runtime_validated: false,
        }
    }
}

impl From<Vec<Value>> for BuiltinFnArgs {
    fn from(v: Vec<Value>) -> Self {
        Self {
            pos: SmallVec::from_vec(v),
            named: None,
            runtime_validated: false,
        }
    }
}

pub trait BuiltinContext {
    fn call(&mut self, func: &Value, args: BuiltinFnArgs) -> WqResult<Value>;
    fn list_enabled_builtins(&self) -> Vec<String>;
}

pub type BuiltinPlainFn = fn(BuiltinFnArgs) -> WqResult<Value>;
pub type BuiltinContextFn = fn(&mut dyn BuiltinContext, BuiltinFnArgs) -> WqResult<Value>;

/// builtin functions
#[derive(Copy, Clone)]
pub enum BuiltinFn {
    Plain(BuiltinPlainFn),
    WithContext(BuiltinContextFn),
}

impl BuiltinFn {
    const fn plain(func: BuiltinPlainFn) -> Self {
        Self::Plain(func)
    }

    const fn with_context(func: BuiltinContextFn) -> Self {
        Self::WithContext(func)
    }

    pub(crate) fn invoke(
        self,
        ctx: &mut dyn BuiltinContext,
        args: BuiltinFnArgs,
    ) -> WqResult<Value> {
        match self {
            Self::Plain(func) => func(args),
            Self::WithContext(func) => func(ctx, args),
        }
    }

    pub(crate) fn as_plain(self) -> Option<BuiltinPlainFn> {
        match self {
            Self::Plain(func) => Some(func),
            Self::WithContext(_) => None,
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum BuiltinDepthSugar {
    None,
    Append {
        non_depth_argc: u16,
    },
    AppendDefaultInt {
        required_argc: u16,
        optional_argc: u16,
        default: i64,
    },
}

#[derive(Clone)]
pub struct Builtins {
    functions: Vec<BuiltinFn>,
    name_to_id: AHashMap<String, usize>,
    enabled: Vec<bool>,
    call_checks: Vec<Option<BuiltinCallCheck>>,
}

impl Default for Builtins {
    fn default() -> Self {
        Self::new()
    }
}

impl Builtins {
    pub fn new() -> Self {
        Self::with_preset(BuiltinPreset::DEFAULT)
    }

    pub fn with_preset(preset: BuiltinPreset) -> Self {
        let mut builtins = Builtins {
            functions: Vec::new(),
            name_to_id: AHashMap::new(),
            enabled: Vec::new(),
            call_checks: Vec::new(),
        };
        builtins.register_functions();
        builtins.call_checks = Self::build_call_checks();
        debug_assert_eq!(
            builtins.functions.len(),
            builtins.call_checks.len(),
            "builtin call checks out of sync"
        );
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
                BuiltinPreset::Pure => group.is_pure(),
                BuiltinPreset::Constrained => group.is_allowed_in_constrained_mode(),
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

    pub fn get_value(&self, name: &str) -> Option<Value> {
        let id = self.get_id(name)?;
        let id = id.try_into().ok()?;
        Some(Value::builtin_function(Arc::<str>::from(name), id))
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

    pub fn list_functions_by_group(&self) -> Vec<(&'static str, Vec<String>)> {
        use std::collections::BTreeMap;
        let mut grouped: BTreeMap<BuiltinGroup, Vec<String>> = BTreeMap::new();
        for (name, &id) in self.name_to_id.iter() {
            if self.is_enabled_id(id as u16) {
                let group = BUILTIN_GROUPS[id];
                grouped.entry(group).or_default().push(name.clone());
            }
        }
        grouped
            .into_iter()
            .map(|(g, mut names)| {
                names.sort();
                (g.name(), names)
            })
            .collect()
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

macro_rules! __builtin_depth_sugar {
    () => {
        BuiltinDepthSugar::None
    };
    ($depth:expr) => {
        $depth
    };
}

macro_rules! __builtin_fn {
    (plain($func:path)) => {
        BuiltinFn::plain($func)
    };

    (with_context($func:path)) => {
        BuiltinFn::with_context($func)
    };
}

macro_rules! __declare_builtins_impl {
    (
        $(
            $(#[$m:meta])*
            (
                $CONST:ident,
                $VAR:ident,
                $name:expr,
                $usage:expr,
                $arity:tt,
                $fn_kind:ident($func:path),
                $group:path
                $(, $depth_sugar:expr)?
            ),
        )+
    ) => {
        #[repr(u16)]
        #[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
        pub enum BuiltinEnum {
            $(
                $(#[$m])*
                $VAR
            ),+
        }

        impl BuiltinEnum {
            pub const fn id(self) -> u16 {
                self as u16
            }

            pub const fn name(self) -> &'static str {
                match self {
                    $(
                        $(#[$m])*
                        BuiltinEnum::$VAR => $name,
                    )+
                }
            }

            pub const fn usage(self) -> &'static str {
                match self {
                    $(
                        $(#[$m])*
                        BuiltinEnum::$VAR => $usage,
                    )+
                }
            }

            pub const fn arity(self) -> &'static str {
                match self {
                    $(
                        $(#[$m])*
                        BuiltinEnum::$VAR => $arity,
                    )+
                }
            }

            pub fn from_id(id: u16) -> Option<Self> {
                match id {
                    $(
                        $(#[$m])*
                        id if id == BuiltinEnum::$VAR as u16 => Some(BuiltinEnum::$VAR),
                    )+
                    _ => None,
                }
            }
        }

        pub const BUILTIN_GROUPS: &[BuiltinGroup] = &[
            $(
                $(#[$m])*
                $group
            ),+
        ];

        pub(crate) const BUILTIN_DEPTH_SUGAR: &[BuiltinDepthSugar] = &[
            $(
                $(#[$m])*
                __builtin_depth_sugar!($($depth_sugar)?),
            )+
        ];

        impl Builtins {
            $(
                $(#[$m])*
                pub const $CONST: u16 = BuiltinEnum::$VAR as u16;
            )+

            pub const NAMES: &'static [&'static str] = &[
                $(
                    $(#[$m])*
                    $name
                ),+
            ];

            pub const USAGES: &'static [&'static str] = &[
                $(
                    $(#[$m])*
                    $usage
                ),+
            ];

            pub const ARITIES: &'static [&'static str] = &[
                $(
                    $(#[$m])*
                    $arity
                ),+
            ];

            pub const ENUMS: &'static [BuiltinEnum] = &[
                $(
                    $(#[$m])*
                    BuiltinEnum::$VAR
                ),+
            ];

            pub(crate) const DEPTH_SUGAR: &'static [BuiltinDepthSugar] = BUILTIN_DEPTH_SUGAR;

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

            pub fn doc_for_name(&self, name: &str) -> Option<crate::doc::DocTopic> {
                self.name_to_id
                    .get(name)
                    .and_then(|id| Self::doc_for_id(*id as u16))
            }

            pub fn doc_for_id(id: u16) -> Option<crate::doc::DocTopic> {
                let builtin = BuiltinEnum::from_id(id)?;
                Some(crate::doc::builtin_topic(builtin))
            }

            #[inline]
            pub(crate) fn depth_sugar_from_id(&self, id: usize) -> BuiltinDepthSugar {
                if !self.enabled.get(id).copied().unwrap_or(false) {
                    return BuiltinDepthSugar::None;
                }

                Self::DEPTH_SUGAR
                    .get(id)
                    .copied()
                    .unwrap_or(BuiltinDepthSugar::None)
            }

            fn register_functions(&mut self) {
                $(
                    $(#[$m])*
                    self.add($name, __builtin_fn!($fn_kind($func)));
                )+
            }
        }
    };
}

declare_builtins! {
    // Core (Pure) =========================================================
    (BFN, Bfn, "bfn", "bfn[]", "0", with_context(core::bfn), BuiltinGroup::CorePure),
    (CHR, Chr, "chr", "chr[xs]", "1", plain(core::chr), BuiltinGroup::CorePure),
    (ORD, Ord, "ord", "ord[xs]", "1", plain(core::ord), BuiltinGroup::CorePure),
    (INT, Int, "int", "int[x], int[x;base]", "1 2", plain(core::int), BuiltinGroup::CorePure),
    (FLOAT, Float, "float", "float[x]", "1", plain(core::float), BuiltinGroup::CorePure),
    (BIN, Bin, "bin", "bin[xs;prefix?]", "1 2", plain(core::bin), BuiltinGroup::CorePure),
    (OCT, Oct, "oct", "oct[xs;prefix?]", "1 2", plain(core::oct), BuiltinGroup::CorePure),
    (HEX, Hex, "hex", "hex[xs;prefix?]", "1 2", plain(core::hex), BuiltinGroup::CorePure),
    (HASH, Hash, "hash", "hash[x]", "1", plain(core::hash), BuiltinGroup::CorePure),
    (RAISE, Raise, "raise", "raise[]; raise[msg]", "0 1", plain(core::raise), BuiltinGroup::CorePure),

    // Core (IO) =========================================================
    (ECHO, Echo, "echo", "echo[value*;`sep]", "0..", plain(core::echo), BuiltinGroup::CoreIO),
    (E, E, "E", "E[value*;`sep]", "0..", plain(core::echo), BuiltinGroup::CoreIO), // alias of echo
    (PRINT, Print, "print", "print[value*]", "0..", plain(core::print), BuiltinGroup::CoreIO),
    (INPUT, Input, "input", "input[prompt?]", "0 1", plain(core::input), BuiltinGroup::CoreIO),
    #[cfg(not(target_arch = "wasm32"))]
    (EXEC, Exec, "exec", "exec[parts+;`stdin;`cwd;`env;`timeout;`check]", "1..", plain(core::exec), BuiltinGroup::Exec),

    // ENCODING =========================================================
    (DECODE, Decode, "decode", "decode[bytes;codec;mode?]", "2 3", plain(encoding::decode), BuiltinGroup::Encoding),
    (ENCODE, Encode, "encode", "encode[text;codec;mode?]", "2 3", plain(encoding::encode), BuiltinGroup::Encoding),
    (VALIDBYTES, ValidBytes, "bytes?", "bytes?[x]", "1", plain(encoding::is_valid_bytes), BuiltinGroup::Encoding),

    // FILE IO =========================================================
    #[cfg(not(target_arch = "wasm32"))]
    {
        (OPEN, Open, "open", "open[path;`r;`w;`a;`t;`c;`cn]", "1", plain(io::open), BuiltinGroup::FileIO),
        (FEXISTS_Q, FexistsQ, "fexists?", "fexists?[path]", "1", plain(io::fexists), BuiltinGroup::FileIO),
        (MKDIR, Mkdir, "mkdir", "mkdir[path]", "1", plain(io::mkdir), BuiltinGroup::FileIO),
        (FSIZE, Fsize, "fsize", "fsize[path]", "1", plain(io::fsize), BuiltinGroup::FileIO),
        (FWRITE, Fwrite, "fwrite", "fwrite[stream;bytes]", "2", plain(io::fwrite), BuiltinGroup::FileIO),
        (FWRITET, Fwritet, "fwritet", "fwritet[stream;text]", "2", plain(io::fwritet), BuiltinGroup::FileIO),
        (FREAD, Fread, "fread", "fread[stream;len?]", "1 2", plain(io::fread), BuiltinGroup::FileIO),
        (FREADT, Freadt, "freadt", "freadt[stream;len?]", "1 2", plain(io::freadt), BuiltinGroup::FileIO),
        (FREADTLN, Freadtln, "freadtln", "freadtln[stream]", "1", plain(io::freadtln), BuiltinGroup::FileIO),
        (FREADTLNS, Freadtlns, "freadtlns", "freadtlns[stream]", "1", plain(io::freadtlns), BuiltinGroup::FileIO),
        (FSEEK, Fseek, "fseek", "fseek[stream;offset;whence?]", "2 3", plain(io::fseek), BuiltinGroup::FileIO),
        (FTELL, Ftell, "ftell", "ftell[stream]", "1", plain(io::ftell), BuiltinGroup::FileIO),
        (FCLOSE, Fclose, "fclose", "fclose[stream]", "1", plain(io::fclose), BuiltinGroup::FileIO),
    },

    // Meta =========================================================
    (LEN, Len, "len", "len[xs]", "1", plain(meta::len), BuiltinGroup::Intrinsic),
    (STRONG_COUNT, StrongCount, "strong_count", "strong_count[x]", "1", plain(meta::strong_count), BuiltinGroup::Meta),
    (SHAPE, Shape, "shape", "shape[xs]", "1", plain(meta::shape), BuiltinGroup::Meta),
    (DEPTH, Depth, "depth", "depth[xs]", "1", plain(meta::depth), BuiltinGroup::Meta),
    (UNIFORM_Q, UniformQ, "uniform?", "uniform?[xs]", "1", plain(meta::is_uniform), BuiltinGroup::Meta),

    // List =========================================================

    (SUM, Sum, "sum", "sum[xs*]", "1..", plain(list::sum), BuiltinGroup::List),
    (PRODUCT, Product, "product", "product[xs*]", "1..", plain(list::product), BuiltinGroup::List),
    (MIN, Min, "min", "min[xs], min[xs;ys+]", "1..", plain(list::min), BuiltinGroup::List),
    (MAX, Max, "max", "max[xs], max[xs;ys+]", "1..", plain(list::max), BuiltinGroup::List),
    (FLATTEN, Flatten, "flatten", "flatten[xs]", "1", plain(list::flatten), BuiltinGroup::List),
    (REVERSE, Reverse, "reverse", "reverse[xs]", "1", plain(list::reverse), BuiltinGroup::List),
    (V, V, "V", "V[xs]", "1", plain(list::reverse), BuiltinGroup::List), // alias of reverse
    (SORT, Sort, "sort", "sort[xs]", "1", plain(list::sort), BuiltinGroup::List),
    (SPLIT, Split, "split", "split[xs;opts?]", "1 2", plain(list::split), BuiltinGroup::List),
    (FIND, Find, "find", "find[xs;elem;threshold?;d?]", "2 3 4", plain(list::find), BuiltinGroup::List, BuiltinDepthSugar::AppendDefaultInt { required_argc: 2, optional_argc: 3, default: 1 }),
    (RFIND, RFind, "rfind", "rfind[xs;elem;threshold?;d?]", "2 3 4", plain(list::rfind), BuiltinGroup::List, BuiltinDepthSugar::AppendDefaultInt { required_argc: 2, optional_argc: 3, default: 1 }),
    (ZIP, Zip, "zip", "zip[xs;ys;d?]", "2 3", plain(list::zip), BuiltinGroup::List, BuiltinDepthSugar::Append { non_depth_argc: 2 }),

    // List Gen =========================================================
    (ALLOC, Alloc, "alloc", "alloc[shape], alloc[shape;x]", "1 2", plain(listgen::alloc), BuiltinGroup::ListGen),
    (TIL, Til, "til", "til[shape]", "1", plain(listgen::til), BuiltinGroup::ListGen),
    (IOTA, Iota, "iota", "iota[shape]", "1", plain(listgen::iota), BuiltinGroup::ListGen),

    (RESHAPE, Reshape, "reshape", "reshape[xs;shape]", "2", plain(listgen::reshape), BuiltinGroup::ListGen),
    (R, R, "R", "R[xs;shape]", "2", plain(listgen::reshape), BuiltinGroup::ListGen), // alias of reshape
    (TRANSPOSE, Transpose, "transpose", "transpose[x;axes?]", "1 2", plain(listgen::transpose::transpose), BuiltinGroup::ListGen),
    (TP, TP, "TP", "TP[x;axes?]", "1 2", plain(listgen::transpose::transpose), BuiltinGroup::ListGen), // alias of transpose

    (REPEAT, Repeat, "repeat", "repeat[xs;n]", "2", plain(listgen::repeat), BuiltinGroup::ListGen),
    (WHERE, Where, "where", "where[xs]", "1", plain(listgen::wq_where), BuiltinGroup::ListGen),
    (Z, Z, "Z", "Z[xs]", "1", plain(listgen::wq_where), BuiltinGroup::ListGen), // alias of where

    // Higher-order =========================================================
    (APPLY, Apply, "apply", "apply[fs;x]", "2", with_context(ho::apply), BuiltinGroup::HigherOrder),
    (A, A, "A", "A[fs;x]", "2", with_context(ho::apply), BuiltinGroup::HigherOrder), // alias of apply
    (MAP, Map, "map", "map[xs;f;d?]", "2 3", with_context(ho::map), BuiltinGroup::HigherOrder, BuiltinDepthSugar::Append { non_depth_argc: 2 }),
    (M, M, "M", "M[xs;f;d?]", "2 3", with_context(ho::map), BuiltinGroup::HigherOrder, BuiltinDepthSugar::Append { non_depth_argc: 2 }), // alias of map
    (FOLD, Fold, "fold", "fold[xs;f;i?]", "2 3", with_context(ho::fold), BuiltinGroup::HigherOrder),
    (REDUCE, Reduce, "reduce", "reduce[xs;f;i?]", "2 3", with_context(ho::fold), BuiltinGroup::HigherOrder), // alias of fold
    (SCAN, Scan, "scan", "scan[xs;f;acc?]", "2 3", with_context(ho::scan), BuiltinGroup::HigherOrder),
    (RSCAN, RScan, "rscan", "rscan[xs;f;acc?]", "2 3", with_context(ho::rscan), BuiltinGroup::HigherOrder),
    (ANY, Any, "any", "any[xs;f;d?]", "2 3", with_context(ho::any), BuiltinGroup::HigherOrder, BuiltinDepthSugar::Append { non_depth_argc: 2 }),
    (ALL, All, "all", "all[xs;f;d?]", "2 3", with_context(ho::all), BuiltinGroup::HigherOrder, BuiltinDepthSugar::Append { non_depth_argc: 2 }),
    (FILTER, Filter, "filter", "filter[xs;f]", "2", with_context(ho::filter), BuiltinGroup::HigherOrder),

    (ZIPW, ZipW, "zipw", "zipw[xs;ys;f;d?]", "3 4", with_context(ho::zipw), BuiltinGroup::HigherOrder, BuiltinDepthSugar::Append { non_depth_argc: 3 }),
    (SPLITW, SplitW, "splitw", "splitw[xs;f;`m]", "2", with_context(ho::splitw), BuiltinGroup::HigherOrder),
    (FINDW, FindW, "findw", "findw[xs;f;threshold?;d?]", "2 3 4", with_context(ho::findw), BuiltinGroup::HigherOrder, BuiltinDepthSugar::AppendDefaultInt { required_argc: 2, optional_argc: 3, default: 1 }),
    (RFINDW, RFindW, "rfindw", "rfindw[xs;f;threshold?;d?]", "2 3 4", with_context(ho::rfindw), BuiltinGroup::HigherOrder, BuiltinDepthSugar::AppendDefaultInt { required_argc: 2, optional_argc: 3, default: 1 }),

    // Dict =========================================================
    (KEYS, Keys, "keys", "keys[dct]", "1", plain(dict::keys), BuiltinGroup::Dict),
    (IDX_TO_KEY, IdxToKey, "itk", "itk[dct;i]", "2", plain(dict::idx_to_key), BuiltinGroup::Dict),
    (KEY_TO_IDX, KeyToIdx, "kti", "kti[dct;k]", "2", plain(dict::key_to_idx), BuiltinGroup::Dict),

    // Set ==========================================================
    (UNIQUE, Unique, "unique", "unique[xs]", "1", plain(set::unique), BuiltinGroup::Set),
    (UNION, Union, "union", "union[xs;ys]", "2", plain(set::union), BuiltinGroup::Set),
    (INTERSECT, Intersect, "intersect", "intersect[xs;ys]", "2", plain(set::intersect), BuiltinGroup::Set),
    (WITHOUT, Without, "without", "without[xs;ys]", "2", plain(set::without), BuiltinGroup::Set),
    (SYMDIFF, Symdiff, "symdiff", "symdiff[xs;ys]", "2", plain(set::symdiff), BuiltinGroup::Set),
    (SUB_Q, SubQ, "sub?", "sub?[xs;ys]", "2", plain(set::subset), BuiltinGroup::Set),
    (SUPER_Q, SuperQ, "super?", "super?[xs;ys]", "2", plain(set::superset), BuiltinGroup::Set),
    (P_SUB_Q, PSubQ, "psub?", "psub?[xs;ys]", "2", plain(set::proper_subset), BuiltinGroup::Set),
    (P_SUPER_Q, PSuperQ, "psuper?", "psuper?[xs;ys]", "2", plain(set::proper_superset), BuiltinGroup::Set),
    (MEMBER_Q, MemberQ, "member?", "member?[xs;ys]", "2", plain(set::member), BuiltinGroup::Set),
    (CART, Cart, "cart", "cart[xs;ys]", "2", plain(set::carproduct), BuiltinGroup::Set),
    (IN_Q, InQ, "in?", "in?[x;xs;d?]", "2 3", plain(set::in_), BuiltinGroup::Set, BuiltinDepthSugar::Append { non_depth_argc: 2 }),
    (HAS_Q, HasQ, "has?", "has?[xs;x;d?]", "2 3", plain(set::has), BuiltinGroup::Set, BuiltinDepthSugar::Append { non_depth_argc: 2 }),
    (DISJOINT_Q, DisjointQ, "disjoint?", "disjoint?[xs;ys]", "2", plain(set::disjoint), BuiltinGroup::Set),
    (MULTIPLICITY, Multiplicity, "multiplicity", "multiplicity[x;xs]", "2", plain(set::multiplicity), BuiltinGroup::Set),

    // Logical ======================================================
    (NOT, Not, "not", "not[xs]", "1", plain(logical::not), BuiltinGroup::Logical),
    (AND, And, "and", "and[xs;ys+]", "2..", plain(logical::and), BuiltinGroup::Logical),
    (OR, Or, "or", "or[xs;ys+]", "2..", plain(logical::or), BuiltinGroup::Logical),
    (XOR, Xor, "xor", "xor[xs;ys+]", "2..", plain(logical::xor), BuiltinGroup::Logical),

    (BNOT, Bnot, "bnot", "bnot[xs]", "1", plain(logical::bnot), BuiltinGroup::Logical),
    (BAND, Band, "band", "band[xs;ys+]", "2..", plain(logical::band), BuiltinGroup::Logical),
    (BOR, Bor, "bor", "bor[xs;ys+]", "2..", plain(logical::bor), BuiltinGroup::Logical),
    (BXOR, Bxor, "bxor", "bxor[xs;ys+]", "2..", plain(logical::bxor), BuiltinGroup::Logical),

    (SHL, Shl, "shl", "shl[xs;shift+]", "2..", plain(logical::shl), BuiltinGroup::Logical),
    (SHR, Shr, "shr", "shr[xs;shift+]", "2..", plain(logical::shr), BuiltinGroup::Logical),

    // Math =========================================================
    (NEG, Neg, "neg", "neg[xs]", "1", plain(math::neg), BuiltinGroup::Math),
    (ABS, Abs, "abs", "abs[xs]", "1", plain(math::abs), BuiltinGroup::Math),
    (SGN, Sgn, "sgn", "sgn[xs]", "1", plain(math::sgn), BuiltinGroup::Math),
    (SQRT, Sqrt, "sqrt", "sqrt[xs]", "1", plain(math::sqrt), BuiltinGroup::Math),
    (EXP, Exp, "exp", "exp[xs]", "1", plain(math::exp), BuiltinGroup::Math),
    (LN, Ln, "ln", "ln[xs]", "1", plain(math::ln), BuiltinGroup::Math),
    (LOG2, Log2, "log2", "log2[xs]", "1", plain(math::log2), BuiltinGroup::Math),
    (LOG10, Log10, "log10", "log10[xs]", "1", plain(math::log10), BuiltinGroup::Math),
    (FLOOR, Floor, "floor", "floor[xs;d?]", "1 2", plain(math::floor), BuiltinGroup::Math),
    (CEIL, Ceil, "ceil", "ceil[xs;d?]", "1 2", plain(math::ceil), BuiltinGroup::Math),
    (ROUND, Round, "round", "round[xs;d?]", "1 2", plain(math::round), BuiltinGroup::Math),

    (SIN, Sin, "sin", "sin[xs]", "1", plain(math::sin), BuiltinGroup::Math),
    (COS, Cos, "cos", "cos[xs]", "1", plain(math::cos), BuiltinGroup::Math),
    (TAN, Tan, "tan", "tan[xs]", "1", plain(math::tan), BuiltinGroup::Math),
    (SEC, Sec, "sec", "sec[xs]", "1", plain(math::sec), BuiltinGroup::Math),
    (CSC, Csc, "csc", "csc[xs]", "1", plain(math::csc), BuiltinGroup::Math),
    (COT, Cot, "cot", "cot[xs]", "1", plain(math::cot), BuiltinGroup::Math),
    (ARCSIN, Arcsin, "arcsin", "arcsin[xs]", "1", plain(math::arcsin), BuiltinGroup::Math),
    (ARCCOS, Arccos, "arccos", "arccos[xs]", "1", plain(math::arccos), BuiltinGroup::Math),
    (ARCTAN, Arctan, "arctan", "arctan[xs]", "1", plain(math::arctan), BuiltinGroup::Math),
    (SINH, Sinh, "sinh", "sinh[xs]", "1", plain(math::sinh), BuiltinGroup::Math),
    (COSH, Cosh, "cosh", "cosh[xs]", "1", plain(math::cosh), BuiltinGroup::Math),
    (TANH, Tanh, "tanh", "tanh[xs]", "1", plain(math::tanh), BuiltinGroup::Math),
    (ARCSINH, Arcsinh, "arcsinh", "arcsinh[xs]", "1", plain(math::arcsinh), BuiltinGroup::Math),
    (ARCCOSH, Arccosh, "arccosh", "arccosh[xs]", "1", plain(math::arccosh), BuiltinGroup::Math),
    (ARCTANH, Arctanh, "arctanh", "arctanh[xs]", "1", plain(math::arctanh), BuiltinGroup::Math),
    (LOG, Log, "log", "log[x;a]", "2", plain(math::log), BuiltinGroup::Math),
    (ARCTAN2, Arctan2, "arctan2", "arctan2[y;x]", "2", plain(math::arctan2), BuiltinGroup::Math),

    (ERF, Erf, "erf", "erf[xs]", "1", plain(math::erf), BuiltinGroup::Math),
    (ERFC, Erfc, "erfc", "erfc[xs]", "1", plain(math::erfc), BuiltinGroup::Math),
    (GAMMA, Gamma, "gamma", "gamma[xs]", "1", plain(math::gamma), BuiltinGroup::Math),
    (LNGAMMA, Lngamma, "lngamma", "lngamma[xs]", "1", plain(math::lngamma), BuiltinGroup::Math),
    (SI, Si, "si", "si[xs]", "1", plain(math::si), BuiltinGroup::Math),
    (CI, Ci, "ci", "ci[xs]", "1", plain(math::ci), BuiltinGroup::Math),
    (EI, Ei, "ei", "ei[xs]", "1", plain(math::ei), BuiltinGroup::Math),
    (EN, En, "en", "en[n;xs]", "2", plain(math::en), BuiltinGroup::Math),
    (ELLPK, Ellpk, "ellpk", "ellpk[xs]", "1", plain(math::ellpk), BuiltinGroup::Math),
    (ELLPE, Ellpe, "ellpe", "ellpe[xs]", "1", plain(math::ellpe), BuiltinGroup::Math),
    (ELLIK, Ellik, "ellik", "ellik[phi;m]", "2", plain(math::ellik), BuiltinGroup::Math),
    (ELLIE, Ellie, "ellie", "ellie[phi;m]", "2", plain(math::ellie), BuiltinGroup::Math),
    (HEAVISIDE, Heaviside, "heaviside", "heaviside[xs]", "1", plain(math::heaviside), BuiltinGroup::Math),
    (DELTA, Delta, "delta", "delta[xs]", "1", plain(math::delta), BuiltinGroup::Math),

    // Rand
    (RAND, Rand, "rand", "rand[]; rand[upper]; rand[lower;upper]", "0 1 2", plain(math::rand), BuiltinGroup::Rand),

    // Complex
    (COMPLEX, Complex, "complex", "complex[re;im]", "2", plain(complex::complex), BuiltinGroup::Complex),
    (RE, Re, "re", "re[x]", "1", plain(complex::real), BuiltinGroup::Complex),
    (IM, Im, "im", "im[x]", "1", plain(complex::imag), BuiltinGroup::Complex),
    (CONJ, Conj, "conj", "conj[x]", "1", plain(complex::conj), BuiltinGroup::Complex),

    // Fraction
    (FRACTION, Fraction, "fraction", "fraction[xs;lim?]", "1 2", plain(fraction::fraction), BuiltinGroup::Fraction),
    (FRACTIONL, Fractionl, "fractionl", "fractionl[xs]", "1", plain(fraction::fractionl), BuiltinGroup::Fraction),

    // CAS
    (EQ, Eq, "eq", "eq[lhs;rhs]", "2", plain(cas::eq), BuiltinGroup::Cas),
    (SIMPLIFY, Simplify, "simplify", "simplify[expr]", "1", plain(cas::simplify), BuiltinGroup::Cas),
    (REWRITE, Rewrite, "rewrite", "rewrite[expr]", "1", plain(cas::rewrite), BuiltinGroup::Cas),
    (NUMERIC, Numeric, "numeric", "numeric[expr]", "1", plain(cas::numeric), BuiltinGroup::Cas),
    (DIFF, Diff, "diff", "diff[expr;var?]", "1 2", plain(cas::diff), BuiltinGroup::Cas),
    (D, D, "D", "D[expr;var?]", "1 2", plain(cas::diff), BuiltinGroup::Cas), // alias of diff
    (SUBSTITUTE, Substitute, "substitute", "substitute[expr;eqs], substitute[expr;var;val]", "2 3", plain(cas::substitute), BuiltinGroup::Cas),
    (EXPAND, Expand, "expand", "expand[expr]", "1", plain(cas::expand), BuiltinGroup::Cas),
    (FACTOR_COMMON, FactorCommon, "factor_common", "factor_common[expr]", "1", plain(cas::factor_common), BuiltinGroup::Cas),
    (FACTOR, Factor, "factor", "factor[expr], factor[expr;var], factor[expr;1], factor[expr;1;var]", "1 2 3", plain(cas::factor_poly), BuiltinGroup::Cas),
    (INTEGRATE, Integrate, "integrate", "integrate[expr], integrate[expr;var], integrate[expr;var;lower;upper]", "1 2 4", plain(cas::integrate), BuiltinGroup::Cas),
    (I, I, "I", "I[expr], I[expr;var], I[expr;var;lower;upper]", "1 2 4", plain(cas::integrate), BuiltinGroup::Cas), // alias of integrate
    (LIMIT, Limit, "limit", "limit[expr;var;point], limit[expr;var;point;dir], limit[expr;vars;points]", "3..", plain(cas::limit), BuiltinGroup::Cas),
    (SOLVE, Solve, "solve", "solve[expr], solve[expr;var], solve[eq;var]", "1 2", plain(cas::solve), BuiltinGroup::Cas),
    (SOLVE_SYSTEM, SolveSystem, "solve_system", "solve_system[eqs;vars]", "2", plain(cas::solve_system), BuiltinGroup::Cas),
    (BRENT, Brent, "brent", "brent[expr;a;b], brent[expr;a;b;tol], brent[expr;a;b;tol;max_iter], brent[eq;a;b]", "3 4 5", plain(cas::brent), BuiltinGroup::Cas),
    (NEWTON, Newton, "newton", "newton[expr;x0], newton[expr;x0;tol], newton[expr;x0;tol;max_iter], newton[eq;x0]", "2 3 4", plain(cas::newton), BuiltinGroup::Cas),

    // String =========================================================
    (STR, Str, "str", "str[x]", "1", plain(string::to_str), BuiltinGroup::Str),
    (GRAPHEMES, Graphemes, "graphemes", "graphemes[s]", "1", plain(string::graphemes), BuiltinGroup::Str),
    (WS_Q, WsQ, "ws?", "ws?[c]", "1", plain(string::is_whitespace), BuiltinGroup::Str),
    (WORDS, Words, "words", "words[s]", "1", plain(string::words), BuiltinGroup::Str),
    (TRIM, Trim, "trim", "trim[s]", "1", plain(string::trim), BuiltinGroup::Str),
    (L_TRIM, LTrim, "ltrim", "ltrim[s]", "1", plain(string::trim_left), BuiltinGroup::Str),
    (R_TRIM, RTrim, "rtrim", "rtrim[s]", "1", plain(string::trim_right), BuiltinGroup::Str),

    // Type =========================================================
    (TYPE, Type, "type", "type[x]", "1", plain(wqtype::type_of), BuiltinGroup::Type),
    (TAG, Tag, "tag", "tag[x]", "1", plain(wqtype::to_tag), BuiltinGroup::Type),
    (BOOL, Bool, "bool", "bool[x]", "1", plain(wqtype::to_bool), BuiltinGroup::Type),
    (CHAR, Char, "char", "char[x]", "1", plain(wqtype::to_char), BuiltinGroup::Type),
    (ATOM_Q, AtomQ, "atom?", "atom?[x]", "1", plain(wqtype::is_atom), BuiltinGroup::Type),
    (UNIT_Q, UnitQ, "unit?", "unit?[x]", "1", plain(wqtype::is_unit), BuiltinGroup::Type),
    (U, U, "U", "U[x]", "1", plain(wqtype::is_unit), BuiltinGroup::Type), // alias of unit?
    (LIST, List, "list", "list[x]", "1", plain(wqtype::to_list), BuiltinGroup::Type),
    (DICT, Dict, "dict", "dict[x]", "1", plain(wqtype::to_dict), BuiltinGroup::Type),

    // Visualization =========================================================
    (SHOWTABLE, Showtable, "showtable", "showtable[table]", "1", plain(viz::show_table), BuiltinGroup::Viz),
    (ASCIIPLOT, Asciiplot, "asciiplot",
        concat!("asciiplot[data+;`size;`width;`height;`xlim;`ylim;",
            "`symbols;`labels;`mode;`axes;`color;`grid;",
            "`samples;`theme;`complex;`ascii;",
            "`title;`xlabel;`ylabel;`caption]"), "1..", with_context(viz::asciiplot), BuiltinGroup::Viz),

    // Intrinsic ====================================================
    (FMT, Fmt, "fmt", "fmt[template;v*]", "1..", plain(string::fmt), BuiltinGroup::Intrinsic),

    (OP_ADD, OpAdd, "+", "+[xs;ys+]", "2..", plain(op::op_add), BuiltinGroup::Intrinsic),
    (OP_SUB, OpSub, "-", "-[x], -[xs;ys+]", "1..", plain(op::op_sub), BuiltinGroup::Intrinsic),
    (OP_MUL, OpMul, "*", "*[xs;ys+]", "2..", plain(op::op_mul), BuiltinGroup::Intrinsic),
    (OP_DIV, OpDiv, "/", "/[xs;ys+]", "2..", plain(op::op_div), BuiltinGroup::Intrinsic),
    (OP_DIVDOT, OpDivDot, "/.", "/.[xs;ys+]", "2..", plain(op::op_divdot), BuiltinGroup::Intrinsic),
    (OP_MOD, OpMod, "%", "%[xs;ys+]", "2..", plain(op::op_mod), BuiltinGroup::Intrinsic),
    (OP_FLOORDIV, OpFloorDiv, "/%", "/%[xs;ys+]", "2..", plain(op::op_floordiv), BuiltinGroup::Intrinsic),
    (OP_POWER, OpPower, "^", "^[xs;ys+]", "2..", plain(op::op_power), BuiltinGroup::Intrinsic),
    (OP_POWERDOT, OpPowerDot, "^.", "^.[xs;ys+]", "2..", plain(op::op_powerdot), BuiltinGroup::Intrinsic),
    (OP_MATMUL, OpMatmul, "**", "**[xs;ys+]", "2..", plain(op::op_matmul), BuiltinGroup::Intrinsic),

    (OP_EQUAL, OpEqual, "=", "=[xs;ys+]", "2..", plain(op::op_equal), BuiltinGroup::Intrinsic),
    (OP_EQUALDOT, OpEqualDot, "=.", "=.[xs;ys+]", "2..", plain(op::op_equaldot), BuiltinGroup::Intrinsic),
    (OP_NOTEQUAL, OpNotEqual, "~", "~[x], ~[xs;ys+]", "1..", plain(op::op_notequal), BuiltinGroup::Intrinsic),
    (OP_NOTEQUALDOT, OpNotEqualDot, "~.", "~.[xs;ys+]", "2..", plain(op::op_notequaldot), BuiltinGroup::Intrinsic),
    (OP_LT, OpLt, "<", "<[xs;ys+]", "2..", plain(op::op_lt), BuiltinGroup::Intrinsic),
    (OP_LTE, OpLte, "<=", "<=[xs;ys+]", "2..", plain(op::op_lte), BuiltinGroup::Intrinsic),
    (OP_GT, OpGt, ">", ">[xs;ys+]", "2..", plain(op::op_gt), BuiltinGroup::Intrinsic),
    (OP_GTE, OpGte, ">=", ">=[xs;ys+]", "2..", plain(op::op_gte), BuiltinGroup::Intrinsic),

    (OP_CAT, OpCat, ",", ",[xs;ys+]", "2..", plain(op::op_cat), BuiltinGroup::Intrinsic),
    (OP_SHARP, OpSharp, "#", "#[x]", "1", plain(op::op_sharp), BuiltinGroup::Intrinsic),

    (OP_BOOLAND, OpBoolAnd, "&|", "&|[xs;ys+]", "2..", plain(op::op_booland), BuiltinGroup::Intrinsic),
    (OP_BOOLOR, OpBoolOr, r"\|", r"\|[xs;ys+]", "2..", plain(op::op_boolor), BuiltinGroup::Intrinsic),
    (OP_BITAND, OpBitAnd, "&", "&[xs;ys+]", "2..", plain(op::op_bitand), BuiltinGroup::Intrinsic),
    (OP_BITOR, OpBitOr, r"\", r"\[xs;ys+]", "2..", plain(op::op_bitor), BuiltinGroup::Intrinsic),
    (OP_BITXOR, OpBitXor, r"^\", r"^\[xs;ys+]", "2..", plain(op::op_bitxor), BuiltinGroup::Intrinsic),
    (OP_SHL, OpShl, "<<", "<<[xs;ys+]", "2..", plain(op::op_shl), BuiltinGroup::Intrinsic),
    (OP_SHR, OpShr, ">>", ">>[xs;ys+]", "2..", plain(op::op_shr), BuiltinGroup::Intrinsic),

}

const ECHO_NAMED_ARGS: &[&str] = &["sep"];
const MAXSPLIT_NAMED_ARGS: &[&str] = &["m"];
#[cfg(not(target_arch = "wasm32"))]
const OPEN_NAMED_ARGS: &[&str] = &["r", "w", "a", "t", "c", "cn"];

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct BuiltinCallCheck {
    src: BuiltinEnum,
    arity: BuiltinCallArity,
    named: BuiltinNamedArgs,
}

impl BuiltinCallCheck {
    fn deny_named(src: BuiltinEnum, arity: BuiltinCallArity) -> Self {
        Self {
            src,
            arity,
            named: BuiltinNamedArgs::Deny,
        }
    }

    fn allow_named(
        src: BuiltinEnum,
        arity: BuiltinCallArity,
        allowed: &'static [&'static str],
    ) -> Self {
        Self {
            src,
            arity,
            named: BuiltinNamedArgs::Allow(allowed),
        }
    }

    fn from_exact_decl(src: BuiltinEnum, arity: &'static str) -> Option<Self> {
        let arity = BuiltinCallArity::parse(arity).expect("valid builtin arity declaration");
        match arity {
            BuiltinCallArity::Exact { .. } => Some(Self::deny_named(src, arity)),
            BuiltinCallArity::AtLeast(_) => None,
        }
    }

    fn validate(self, args: &BuiltinFnArgs) -> WqResult<()> {
        self.arity.validate(self.src, args)?;
        match self.named {
            BuiltinNamedArgs::Deny => deny_named_args(args, self.src),
            BuiltinNamedArgs::Allow(allowed) => check_named_args(args, self.src, allowed),
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum BuiltinNamedArgs {
    Deny,
    Allow(&'static [&'static str]),
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum BuiltinCallArity {
    Exact { mask: u128 },
    AtLeast(usize),
}

impl BuiltinCallArity {
    fn parse(raw: &str) -> Option<Self> {
        if let Some(min) = raw.strip_suffix("..") {
            return min.trim().parse().ok().map(Self::AtLeast);
        }

        let mut mask = 0u128;
        for part in raw.split_whitespace() {
            let n: usize = part.parse().ok()?;
            if n >= u128::BITS as usize {
                return None;
            }
            mask |= 1u128 << n;
        }
        (mask != 0).then_some(Self::Exact { mask })
    }

    fn exact(arities: impl AsRef<[usize]>) -> Self {
        let mut mask = 0u128;
        for &n in arities.as_ref() {
            debug_assert!(n < u128::BITS as usize, "builtin arity too large");
            mask |= 1u128 << n;
        }
        Self::Exact { mask }
    }

    fn contains(self, n: usize) -> bool {
        match self {
            Self::Exact { mask } if n < u128::BITS as usize => mask & (1u128 << n) != 0,
            Self::Exact { .. } => false,
            Self::AtLeast(min) => n >= min,
        }
    }

    fn validate(self, builtin: BuiltinEnum, args: &[Value]) -> WqResult<()> {
        let n = args.len();
        if self.contains(n) {
            return Ok(());
        }

        match self {
            Self::Exact { mask } => Err(exact_arity_error(builtin, mask, n)),
            Self::AtLeast(min) => Err(WqError::new(WqErrorType::Arity)
                .src(builtin)
                .msg(format!("expected {min} or more args, got {n}"))
                .attach_note(builtin.usage())),
        }
    }
}

impl Builtins {
    fn build_call_checks() -> Vec<Option<BuiltinCallCheck>> {
        Self::ENUMS
            .iter()
            .copied()
            .map(runtime_call_check_for)
            .collect()
    }

    pub(crate) fn validate_runtime_call_args(
        &self,
        id: u16,
        args: &BuiltinFnArgs,
    ) -> WqResult<bool> {
        let Some(check) = self.call_checks.get(usize::from(id)).copied().flatten() else {
            return Ok(false);
        };

        check.validate(args)?;
        Ok(true)
    }
}

fn runtime_call_check_for(builtin: BuiltinEnum) -> Option<BuiltinCallCheck> {
    use BuiltinEnum as B;

    let src = runtime_validation_source(builtin);
    let check = match builtin {
        B::Echo | B::E => {
            BuiltinCallCheck::allow_named(src, BuiltinCallArity::AtLeast(0), ECHO_NAMED_ARGS)
        }
        #[cfg(not(target_arch = "wasm32"))]
        B::Open => {
            BuiltinCallCheck::allow_named(src, BuiltinCallArity::exact([1]), OPEN_NAMED_ARGS)
        }
        B::Split => {
            BuiltinCallCheck::allow_named(src, BuiltinCallArity::exact([1, 2]), MAXSPLIT_NAMED_ARGS)
        }
        B::SplitW => {
            BuiltinCallCheck::allow_named(src, BuiltinCallArity::exact([2]), MAXSPLIT_NAMED_ARGS)
        }

        B::Print
        | B::Sum
        | B::Product
        | B::Min
        | B::Max
        | B::Limit
        | B::And
        | B::Or
        | B::Xor
        | B::Band
        | B::Bor
        | B::Bxor
        | B::Shl
        | B::Shr
        | B::Apply
        | B::A
        | B::Fmt
        | B::Asciiplot
        | B::OpAdd
        | B::OpSub
        | B::OpMul
        | B::OpDiv
        | B::OpDivDot
        | B::OpMod
        | B::OpFloorDiv
        | B::OpPower
        | B::OpPowerDot
        | B::OpMatmul
        | B::OpEqual
        | B::OpEqualDot
        | B::OpNotEqual
        | B::OpNotEqualDot
        | B::OpLt
        | B::OpLte
        | B::OpGt
        | B::OpGte
        | B::OpCat
        | B::OpBoolAnd
        | B::OpBoolOr
        | B::OpBitAnd
        | B::OpBitOr
        | B::OpBitXor
        | B::OpShl
        | B::OpShr => return None,
        #[cfg(not(target_arch = "wasm32"))]
        B::Exec => return None,

        _ => return BuiltinCallCheck::from_exact_decl(src, builtin.arity()),
    };

    Some(check)
}

fn runtime_validation_source(builtin: BuiltinEnum) -> BuiltinEnum {
    use BuiltinEnum as B;

    match builtin {
        B::E => B::Echo,
        B::V => B::Reverse,
        B::R => B::Reshape,
        B::TP => B::Transpose,
        B::Z => B::Where,
        B::M => B::Map,
        B::Reduce => B::Fold,
        B::D => B::Diff,
        B::I => B::Integrate,
        B::U => B::UnitQ,
        other => other,
    }
}

fn exact_arity_error(builtin: BuiltinEnum, mask: u128, n: usize) -> WqError {
    let mut arities = Vec::new();
    let mut remaining = mask;
    let mut arity = 0usize;
    while remaining != 0 {
        if remaining & 1 != 0 {
            arities.push(arity);
        }
        remaining >>= 1;
        arity += 1;
    }

    let expected = match arities.as_slice() {
        [] => return WqError::new(WqErrorType::Arity).src(builtin),
        [one] => format!("{one}"),
        [a, b] => format!("{a} or {b}"),
        many => {
            let mut parts: Vec<String> = many.iter().map(|x| x.to_string()).collect();
            let last = parts
                .pop()
                .expect("exact arity error has at least one arity");
            if parts.is_empty() {
                last
            } else {
                format!("{}, or {}", parts.join(", "), last)
            }
        }
    };
    let plural = if arities.len() == 1 && arities[0] == 1 {
        "arg"
    } else {
        "args"
    };

    WqError::new(WqErrorType::Arity)
        .src(builtin)
        .msg(format!("expected {expected} {plural}, got {n}"))
        .attach_note(builtin.usage())
}

impl std::fmt::Display for BuiltinEnum {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "bfn '{}'", self.name())
    }
}

#[inline]
fn fold_value<F>(src: BuiltinEnum, args: BuiltinFnArgs, f: F) -> WqResult<Value>
where
    F: Fn(&Value, &Value) -> WqResult<Value>,
{
    if args.len() < 2 {
        return Err(WqError::new(WqErrorType::Arity)
            .msg(format!("expected 2 or more args, got {}", args.len())));
    }
    let mut iter = args.into_iter();
    let mut acc = iter.next().unwrap();
    for v in iter {
        acc = f(&acc, &v).map_err(|e| e.src(src))?;
    }
    Ok(acc)
}

#[inline]
pub(super) fn type_mismatch(
    builtin: BuiltinEnum,
    pos: usize,
    expected: &str,
    got: &Value,
) -> WqError {
    WqError::new(WqErrorType::Domain)
        .src(builtin)
        .msg(format!("expected {expected}"))
        .at_arg(pos)
        .got1(got)
        .attach_note(format!("usage: {}", builtin.usage()))
}

#[inline]
pub(super) fn check_arity(
    builtin: BuiltinEnum,
    arity: impl AsRef<[usize]>,
    args: &BuiltinFnArgs,
) -> WqResult<()> {
    if args.runtime_validated() {
        return Ok(());
    }

    // Deny named args first so the error message is clearer.
    deny_named_args(args, builtin)?;
    check_arity_inner(builtin, arity, args)
}

/// Like check_arity but allows named args.
#[inline]
pub(super) fn check_arity_named(
    builtin: BuiltinEnum,
    arity: impl AsRef<[usize]>,
    args: &BuiltinFnArgs,
    allowed: &[&str],
) -> WqResult<()> {
    if args.runtime_validated() {
        return Ok(());
    }

    check_arity_inner(builtin, arity, args)?;
    check_named_args(args, builtin, allowed)
}

/// Validate that all provided named args are in the allowed list.
#[inline]
pub(super) fn check_named_args(
    args: &BuiltinFnArgs,
    builtin: BuiltinEnum,
    allowed: &[&str],
) -> WqResult<()> {
    if args.runtime_validated() {
        return Ok(());
    }

    if let Some(named) = &args.named {
        for (name, _) in named {
            if !allowed.contains(&name.as_ref()) {
                return Err(WqError::new(WqErrorType::Arity)
                    .src(builtin)
                    .msg(format!("unknown named argument '{}'", name)));
            }
        }
    }
    Ok(())
}

#[inline]
fn check_arity_inner(
    builtin: BuiltinEnum,
    arity: impl AsRef<[usize]>,
    args: &[Value],
) -> WqResult<()> {
    let arity = arity.as_ref();
    let n = args.len();
    if !arity.contains(&n) {
        let expected = match arity {
            [] => return Ok(()),
            [one] => format!("{one}"),
            [a, b] => format!("{a} or {b}"),
            many => {
                let mut parts: Vec<String> = many.iter().map(|x| x.to_string()).collect();
                let last = parts.pop().unwrap();
                if parts.is_empty() {
                    last
                } else {
                    format!("{}, or {}", parts.join(", "), last)
                }
            }
        };
        let plural = if arity.len() == 1 && arity[0] == 1 {
            "arg"
        } else {
            "args"
        };
        return Err(WqError::new(WqErrorType::Arity)
            .src(builtin)
            .msg(format!("expected {expected} {plural}, got {n}"))
            .attach_note(builtin.usage()));
    }
    Ok(())
}

#[inline]
fn deny_named_args(args: &BuiltinFnArgs, builtin: BuiltinEnum) -> WqResult<()> {
    if args.runtime_validated() {
        return Ok(());
    }

    if let Some(named) = &args.named
        && let Some((name, _)) = named.first()
    {
        return Err(WqError::new(WqErrorType::Arity)
            .src(builtin)
            .msg(format!("unexpected named argument '{}'", name)));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use smallvec::SmallVec;

    use super::*;
    use crate::value::into_wq_string;

    #[test]
    fn runtime_call_checks_preserve_implementation_arities() {
        let builtins = Builtins::new();

        assert!(
            builtins
                .validate_runtime_call_args(Builtins::RAISE, &BuiltinFnArgs::new())
                .expect("raise runtime validation should succeed")
        );
        assert!(
            builtins
                .validate_runtime_call_args(
                    Builtins::SUBSTITUTE,
                    &BuiltinFnArgs::from(vec![Value::Int(1), Value::Int(2)]),
                )
                .expect("substitute runtime validation should succeed")
        );
        assert!(
            builtins
                .validate_runtime_call_args(
                    Builtins::ARCTAN2,
                    &BuiltinFnArgs::from(vec![Value::Int(1), Value::Int(2)]),
                )
                .expect("arctan2 runtime validation should succeed")
        );

        let err = builtins
            .validate_runtime_call_args(Builtins::ARCTAN2, &BuiltinFnArgs::from(Value::Int(1)))
            .expect_err("arctan2 with one arg should fail runtime validation");
        assert_eq!(err.msg.as_deref(), Some("expected 2 args, got 1"));
    }

    #[test]
    fn runtime_call_checks_skip_custom_builtins() {
        let builtins = Builtins::new();

        for id in [
            Builtins::PRINT,
            Builtins::SUM,
            Builtins::LIMIT,
            Builtins::FMT,
            Builtins::ASCIIPLOT,
            Builtins::OP_ADD,
        ] {
            assert!(
                !builtins
                    .validate_runtime_call_args(id, &BuiltinFnArgs::new())
                    .expect("custom builtin runtime validation should not error")
            );
        }
    }

    #[test]
    fn runtime_call_checks_use_canonical_alias_sources() {
        let builtins = Builtins::new();

        let err = builtins
            .validate_runtime_call_args(Builtins::V, &BuiltinFnArgs::new())
            .expect_err("V with zero args should fail runtime validation");
        assert_eq!(err.src.as_deref(), Some("bfn 'reverse'"));
    }

    #[test]
    fn runtime_call_checks_validate_simple_named_args() {
        let builtins = Builtins::new();
        let args = BuiltinFnArgs::with_named(
            SmallVec::new(),
            vec![(Arc::<str>::from("sep"), into_wq_string(","))],
        );

        assert!(
            builtins
                .validate_runtime_call_args(Builtins::E, &args)
                .expect("E named runtime validation should succeed")
        );

        let bad_args = BuiltinFnArgs::with_named(
            SmallVec::new(),
            vec![(Arc::<str>::from("bad"), Value::Int(1))],
        );
        let err = builtins
            .validate_runtime_call_args(Builtins::E, &bad_args)
            .expect_err("unknown named arg should fail runtime validation");
        assert_eq!(err.src.as_deref(), Some("bfn 'echo'"));
        assert_eq!(err.msg.as_deref(), Some("unknown named argument 'bad'"));
    }

    #[test]
    fn runtime_validated_args_skip_body_shape_checks() {
        let mut args = BuiltinFnArgs::new();
        args.mark_runtime_validated();
        assert!(check_arity(BuiltinEnum::Len, [1], &args).is_ok());

        let mut named_args = BuiltinFnArgs::with_named(
            SmallVec::new(),
            vec![(Arc::<str>::from("bad"), Value::Int(1))],
        );
        named_args.mark_runtime_validated();
        assert!(check_named_args(&named_args, BuiltinEnum::Echo, &[]).is_ok());
    }
}
