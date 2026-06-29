mod cas;
mod complex;
mod core;
mod dict;
mod encoding;
mod fold;
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
/// Positional args: `SmallVec<[Value; 4]>` -- zero heap for ≤4 args.
/// Named args: `Option<Vec<(Arc<str>, Value)>>` -- heap-allocated, None when
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
    pub(crate) fn named_items(&self) -> &[(Arc<str>, Value)] {
        self.named.as_deref().unwrap_or(&[])
    }
    pub fn push(&mut self, v: Value) {
        self.runtime_validated = false;
        self.pos.push(v)
    }

    pub(crate) fn from_cloned_slice(values: &[Value]) -> Self {
        Self {
            pos: values.iter().cloned().collect(),
            named: None,
            runtime_validated: false,
        }
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

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum BuiltinCallArity {
    Exact { mask: u128 },
    AtLeast(usize),
}

impl BuiltinCallArity {
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

impl std::fmt::Display for BuiltinCallArity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            BuiltinCallArity::Exact { mask } => {
                let mut remaining = mask;
                let mut arity = 0usize;
                let mut needs_space = false;
                while remaining != 0 {
                    if remaining & 1 != 0 {
                        if needs_space {
                            write!(f, " ")?;
                        }
                        write!(f, "{arity}")?;
                        needs_space = true;
                    }
                    remaining >>= 1;
                    arity += 1;
                }
                Ok(())
            }
            BuiltinCallArity::AtLeast(min) => write!(f, "{min}.."),
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum BuiltinNamedArgs {
    Deny,
    Allow(&'static [&'static str]),
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum BuiltinValidation {
    Fast,
    Defer,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct BuiltinSignature {
    arity: BuiltinCallArity,
    named: BuiltinNamedArgs,
    validation: BuiltinValidation,
    source: Option<BuiltinEnum>,
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

macro_rules! arity {
    ($min:literal ..) => {
        BuiltinCallArity::AtLeast($min)
    };
    ($first:literal $(, $rest:literal)* $(,)?) => {
        BuiltinCallArity::Exact {
            mask: (1u128 << $first) $(| (1u128 << $rest))*,
        }
    };
}

macro_rules! sig {
    ($arity:expr) => {
        BuiltinSignature {
            arity: $arity,
            named: BuiltinNamedArgs::Deny,
            validation: BuiltinValidation::Fast,
            source: None,
        }
    };
    ($arity:expr, defer) => {
        BuiltinSignature {
            arity: $arity,
            named: BuiltinNamedArgs::Deny,
            validation: BuiltinValidation::Defer,
            source: None,
        }
    };
    ($arity:expr, named $named:expr) => {
        BuiltinSignature {
            arity: $arity,
            named: BuiltinNamedArgs::Allow($named),
            validation: BuiltinValidation::Fast,
            source: None,
        }
    };
    ($arity:expr, alias $source:ident) => {
        BuiltinSignature {
            arity: $arity,
            named: BuiltinNamedArgs::Deny,
            validation: BuiltinValidation::Fast,
            source: Some(BuiltinEnum::$source),
        }
    };
    ($arity:expr, named $named:expr, alias $source:ident) => {
        BuiltinSignature {
            arity: $arity,
            named: BuiltinNamedArgs::Allow($named),
            validation: BuiltinValidation::Fast,
            source: Some(BuiltinEnum::$source),
        }
    };
    ($arity:expr, alias $source:ident, named $named:expr) => {
        sig!($arity, named $named, alias $source)
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
                $signature:expr,
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

            pub(crate) const fn signature(self) -> BuiltinSignature {
                match self {
                    $(
                        $(#[$m])*
                        BuiltinEnum::$VAR => $signature,
                    )+
                }
            }

            pub const fn arity(self) -> BuiltinCallArity {
                self.signature().arity
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

            pub(crate) fn canonical(self) -> Self {
                self.signature().source.unwrap_or(self)
            }

            pub(crate) fn discard_fn(self) -> Option<BuiltinContextFn> {
                match self.canonical() {
                    BuiltinEnum::Apply => Some(ho::apply_discard),
                    BuiltinEnum::Map => Some(ho::map_discard),
                    BuiltinEnum::Filter => Some(ho::filter_discard),
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

            pub(crate) const SIGNATURES: &'static [BuiltinSignature] = &[
                $(
                    $(#[$m])*
                    $signature
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
            pub fn arity_from_id(id: u16) -> Option<BuiltinCallArity> {
                Self::SIGNATURES.get(id as usize).map(|signature| signature.arity)
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
            pub(crate) fn has_discard_fn_from_id(id: u16) -> bool {
                BuiltinEnum::from_id(id)
                    .and_then(BuiltinEnum::discard_fn)
                    .is_some()
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
    (BFN, Bfn, "bfn", "bfn[]", sig!(arity!(0)), with_context(core::bfn), BuiltinGroup::CorePure),
    (CHR, Chr, "chr", "chr[xs]", sig!(arity!(1)), plain(core::chr), BuiltinGroup::CorePure),
    (ORD, Ord, "ord", "ord[xs]", sig!(arity!(1)), plain(core::ord), BuiltinGroup::CorePure),
    (INT, Int, "int", "int[x], int[x;base]", sig!(arity!(1, 2)), plain(core::int), BuiltinGroup::CorePure),
    (FLOAT, Float, "float", "float[x]", sig!(arity!(1)), plain(core::float), BuiltinGroup::CorePure),
    (BIN, Bin, "bin", "bin[xs;prefix?]", sig!(arity!(1, 2)), plain(core::bin), BuiltinGroup::CorePure),
    (OCT, Oct, "oct", "oct[xs;prefix?]", sig!(arity!(1, 2)), plain(core::oct), BuiltinGroup::CorePure),
    (HEX, Hex, "hex", "hex[xs;prefix?]", sig!(arity!(1, 2)), plain(core::hex), BuiltinGroup::CorePure),
    (HASH, Hash, "hash", "hash[x]", sig!(arity!(1)), plain(core::hash), BuiltinGroup::CorePure),
    (RAISE, Raise, "raise", "raise[]; raise[msg]", sig!(arity!(0, 1)), plain(core::raise), BuiltinGroup::CorePure),

    // Core (IO) =========================================================
    (ECHO, Echo, "echo", "echo[value*;`sep]", sig!(arity!(0..), named ECHO_NAMED_ARGS), plain(core::echo), BuiltinGroup::CoreIO),
    (E, E, "E", "E[value*;`sep]", sig!(arity!(0..), named ECHO_NAMED_ARGS, alias Echo), plain(core::echo), BuiltinGroup::CoreIO), // alias of echo
    (PRINT, Print, "print", "print[value*]", sig!(arity!(0..)), plain(core::print), BuiltinGroup::CoreIO),
    (INPUT, Input, "input", "input[prompt?]", sig!(arity!(0, 1)), plain(core::input), BuiltinGroup::CoreIO),
    #[cfg(not(target_arch = "wasm32"))]
    (EXEC, Exec, "exec", "exec[parts+;`stdin;`cwd;`env;`timeout;`check]", sig!(arity!(1..), named EXEC_NAMED_ARGS), plain(core::exec), BuiltinGroup::Exec),

    // ENCODING =========================================================
    (DECODE, Decode, "decode", "decode[bytes;codec;mode?]", sig!(arity!(2, 3)), plain(encoding::decode), BuiltinGroup::Encoding),
    (ENCODE, Encode, "encode", "encode[text;codec;mode?]", sig!(arity!(2, 3)), plain(encoding::encode), BuiltinGroup::Encoding),
    (VALIDBYTES, ValidBytes, "bytes?", "bytes?[x]", sig!(arity!(1)), plain(encoding::is_valid_bytes), BuiltinGroup::Encoding),

    // FILE IO =========================================================
    #[cfg(not(target_arch = "wasm32"))]
    {
        (OPEN, Open, "open", "open[path;`r;`w;`a;`t;`c;`cn]", sig!(arity!(1), named OPEN_NAMED_ARGS), plain(io::open), BuiltinGroup::FileIO),
        (FEXISTS_Q, FexistsQ, "fexists?", "fexists?[path]", sig!(arity!(1)), plain(io::fexists), BuiltinGroup::FileIO),
        (MKDIR, Mkdir, "mkdir", "mkdir[path]", sig!(arity!(1)), plain(io::mkdir), BuiltinGroup::FileIO),
        (FSIZE, Fsize, "fsize", "fsize[path]", sig!(arity!(1)), plain(io::fsize), BuiltinGroup::FileIO),
        (FWRITE, Fwrite, "fwrite", "fwrite[stream;bytes]", sig!(arity!(2)), plain(io::fwrite), BuiltinGroup::FileIO),
        (FWRITET, Fwritet, "fwritet", "fwritet[stream;text]", sig!(arity!(2)), plain(io::fwritet), BuiltinGroup::FileIO),
        (FREAD, Fread, "fread", "fread[stream;len?]", sig!(arity!(1, 2)), plain(io::fread), BuiltinGroup::FileIO),
        (FREADT, Freadt, "freadt", "freadt[stream;len?]", sig!(arity!(1, 2)), plain(io::freadt), BuiltinGroup::FileIO),
        (FREADTLN, Freadtln, "freadtln", "freadtln[stream]", sig!(arity!(1)), plain(io::freadtln), BuiltinGroup::FileIO),
        (FREADTLNS, Freadtlns, "freadtlns", "freadtlns[stream]", sig!(arity!(1)), plain(io::freadtlns), BuiltinGroup::FileIO),
        (FSEEK, Fseek, "fseek", "fseek[stream;offset;whence?]", sig!(arity!(2, 3)), plain(io::fseek), BuiltinGroup::FileIO),
        (FTELL, Ftell, "ftell", "ftell[stream]", sig!(arity!(1)), plain(io::ftell), BuiltinGroup::FileIO),
        (FCLOSE, Fclose, "fclose", "fclose[stream]", sig!(arity!(1)), plain(io::fclose), BuiltinGroup::FileIO),
    },

    // Meta =========================================================
    (LEN, Len, "len", "len[xs]", sig!(arity!(1)), plain(meta::len), BuiltinGroup::Intrinsic),
    (STRONG_COUNT, StrongCount, "strong_count", "strong_count[x]", sig!(arity!(1)), plain(meta::strong_count), BuiltinGroup::Meta),
    (SHAPE, Shape, "shape", "shape[xs]", sig!(arity!(1)), plain(meta::shape), BuiltinGroup::Meta),
    (DEPTH, Depth, "depth", "depth[xs]", sig!(arity!(1)), plain(meta::depth), BuiltinGroup::Meta),
    (UNIFORM_Q, UniformQ, "uniform?", "uniform?[xs]", sig!(arity!(1)), plain(meta::is_uniform), BuiltinGroup::Meta),

    // List =========================================================
    (SUM, Sum, "sum", "sum[xs*]", sig!(arity!(0..)), plain(list::sum), BuiltinGroup::List),
    (PRODUCT, Product, "product", "product[xs*]", sig!(arity!(0..)), plain(list::product), BuiltinGroup::List),
    (MIN, Min, "min", "min[xs], min[xs;ys+]", sig!(arity!(1..)), plain(list::min), BuiltinGroup::List),
    (MAX, Max, "max", "max[xs], max[xs;ys+]", sig!(arity!(1..)), plain(list::max), BuiltinGroup::List),
    (FLATTEN, Flatten, "flatten", "flatten[xs]", sig!(arity!(1)), plain(list::flatten), BuiltinGroup::List),
    (REVERSE, Reverse, "reverse", "reverse[xs]", sig!(arity!(1)), plain(list::reverse), BuiltinGroup::List),
    (V, V, "V", "V[xs]", sig!(arity!(1), alias Reverse), plain(list::reverse), BuiltinGroup::List), // alias of reverse
    (SORT, Sort, "sort", "sort[xs]", sig!(arity!(1)), plain(list::sort), BuiltinGroup::List),
    (SPLIT, Split, "split", "split[xs;opts?]", sig!(arity!(1, 2), named MAXSPLIT_NAMED_ARGS), plain(list::split), BuiltinGroup::List),
    (FIND, Find, "find", "find[xs;elem;threshold?;d?]", sig!(arity!(2, 3, 4)), plain(list::find), BuiltinGroup::List, BuiltinDepthSugar::AppendDefaultInt { required_argc: 2, optional_argc: 3, default: 1 }),
    (RFIND, RFind, "rfind", "rfind[xs;elem;threshold?;d?]", sig!(arity!(2, 3, 4)), plain(list::rfind), BuiltinGroup::List, BuiltinDepthSugar::AppendDefaultInt { required_argc: 2, optional_argc: 3, default: 1 }),
    (ZIP, Zip, "zip", "zip[xs;ys;d?]", sig!(arity!(2, 3)), plain(list::zip), BuiltinGroup::List, BuiltinDepthSugar::Append { non_depth_argc: 2 }),

    // List Gen =========================================================
    (ALLOC, Alloc, "alloc", "alloc[shape], alloc[shape;x]", sig!(arity!(1, 2)), plain(listgen::alloc), BuiltinGroup::ListGen),
    (TIL, Til, "til", "til[shape]", sig!(arity!(1)), plain(listgen::til), BuiltinGroup::ListGen),
    (IOTA, Iota, "iota", "iota[shape]", sig!(arity!(1)), plain(listgen::iota), BuiltinGroup::ListGen),
    (RANGE, Range, "range", "range[start;end], range[start;end;step]", sig!(arity!(2, 3)), plain(listgen::range), BuiltinGroup::ListGen),

    (RESHAPE, Reshape, "reshape", "reshape[xs;shape]", sig!(arity!(2)), plain(listgen::reshape), BuiltinGroup::ListGen),
    (R, R, "R", "R[xs;shape]", sig!(arity!(2), alias Reshape), plain(listgen::reshape), BuiltinGroup::ListGen), // alias of reshape
    (TRANSPOSE, Transpose, "transpose", "transpose[x;axes?]", sig!(arity!(1, 2)), plain(listgen::transpose::transpose), BuiltinGroup::ListGen),
    (TP, TP, "TP", "TP[x;axes?]", sig!(arity!(1, 2), alias Transpose), plain(listgen::transpose::transpose), BuiltinGroup::ListGen), // alias of transpose

    (REPEAT, Repeat, "repeat", "repeat[xs;n]", sig!(arity!(2)), plain(listgen::repeat), BuiltinGroup::ListGen),
    (WHERE, Where, "where", "where[xs]", sig!(arity!(1)), plain(listgen::wq_where), BuiltinGroup::ListGen),
    (Z, Z, "Z", "Z[xs]", sig!(arity!(1), alias Where), plain(listgen::wq_where), BuiltinGroup::ListGen), // alias of where

    // Higher-order =========================================================
    (APPLY, Apply, "apply", "apply[fs;x]", sig!(arity!(2)), with_context(ho::apply), BuiltinGroup::HigherOrder),
    (MAP, Map, "map", "map[xs;f;d?]", sig!(arity!(2, 3)), with_context(ho::map), BuiltinGroup::HigherOrder, BuiltinDepthSugar::Append { non_depth_argc: 2 }),
    (M, M, "M", "M[xs;f;d?]", sig!(arity!(2, 3), alias Map), with_context(ho::map), BuiltinGroup::HigherOrder, BuiltinDepthSugar::Append { non_depth_argc: 2 }), // alias of map
    (FOLD, Fold, "fold", "fold[xs;f;i?]", sig!(arity!(2, 3)), with_context(ho::fold), BuiltinGroup::HigherOrder),
    (REDUCE, Reduce, "reduce", "reduce[xs;f;i?]", sig!(arity!(2, 3), alias Fold), with_context(ho::fold), BuiltinGroup::HigherOrder), // alias of fold
    (SCAN, Scan, "scan", "scan[xs;f;acc?]", sig!(arity!(2, 3)), with_context(ho::scan), BuiltinGroup::HigherOrder),
    (RSCAN, RScan, "rscan", "rscan[xs;f;acc?]", sig!(arity!(2, 3)), with_context(ho::rscan), BuiltinGroup::HigherOrder),
    (ANY, Any, "any", "any[xs;f;d?]", sig!(arity!(2, 3)), with_context(ho::any), BuiltinGroup::HigherOrder, BuiltinDepthSugar::Append { non_depth_argc: 2 }),
    (ALL, All, "all", "all[xs;f;d?]", sig!(arity!(2, 3)), with_context(ho::all), BuiltinGroup::HigherOrder, BuiltinDepthSugar::Append { non_depth_argc: 2 }),
    (FILTER, Filter, "filter", "filter[xs;f]", sig!(arity!(2)), with_context(ho::filter), BuiltinGroup::HigherOrder),

    (ZIPW, ZipW, "zipw", "zipw[xs;ys;f;d?]", sig!(arity!(3, 4)), with_context(ho::zipw), BuiltinGroup::HigherOrder, BuiltinDepthSugar::Append { non_depth_argc: 3 }),
    (SPLITW, SplitW, "splitw", "splitw[xs;f;`m]", sig!(arity!(2), named MAXSPLIT_NAMED_ARGS), with_context(ho::splitw), BuiltinGroup::HigherOrder),
    (FINDW, FindW, "findw", "findw[xs;f;threshold?;d?]", sig!(arity!(2, 3, 4)), with_context(ho::findw), BuiltinGroup::HigherOrder, BuiltinDepthSugar::AppendDefaultInt { required_argc: 2, optional_argc: 3, default: 1 }),
    (RFINDW, RFindW, "rfindw", "rfindw[xs;f;threshold?;d?]", sig!(arity!(2, 3, 4)), with_context(ho::rfindw), BuiltinGroup::HigherOrder, BuiltinDepthSugar::AppendDefaultInt { required_argc: 2, optional_argc: 3, default: 1 }),

    // Dict =========================================================
    (KEYS, Keys, "keys", "keys[dct]", sig!(arity!(1)), plain(dict::keys), BuiltinGroup::Dict),
    (IDX_TO_KEY, IdxToKey, "itk", "itk[dct;i]", sig!(arity!(2)), plain(dict::idx_to_key), BuiltinGroup::Dict),
    (KEY_TO_IDX, KeyToIdx, "kti", "kti[dct;k]", sig!(arity!(2)), plain(dict::key_to_idx), BuiltinGroup::Dict),

    // Set ==========================================================
    (UNIQUE, Unique, "unique", "unique[xs]", sig!(arity!(1)), plain(set::unique), BuiltinGroup::Set),
    (COUNTS, Counts, "counts", "counts[xs]", sig!(arity!(1)), plain(set::counts), BuiltinGroup::Set),
    (UNION, Union, "union", "union[xs;ys]", sig!(arity!(2)), plain(set::union), BuiltinGroup::Set),
    (INTERSECT, Intersect, "intersect", "intersect[xs;ys]", sig!(arity!(2)), plain(set::intersect), BuiltinGroup::Set),
    (WITHOUT, Without, "without", "without[xs;ys]", sig!(arity!(2)), plain(set::without), BuiltinGroup::Set),
    (SYMDIFF, Symdiff, "symdiff", "symdiff[xs;ys]", sig!(arity!(2)), plain(set::symdiff), BuiltinGroup::Set),
    (SUB_Q, SubQ, "sub?", "sub?[xs;ys]", sig!(arity!(2)), plain(set::subset), BuiltinGroup::Set),
    (SUPER_Q, SuperQ, "super?", "super?[xs;ys]", sig!(arity!(2)), plain(set::superset), BuiltinGroup::Set),
    (P_SUB_Q, PSubQ, "psub?", "psub?[xs;ys]", sig!(arity!(2)), plain(set::proper_subset), BuiltinGroup::Set),
    (P_SUPER_Q, PSuperQ, "psuper?", "psuper?[xs;ys]", sig!(arity!(2)), plain(set::proper_superset), BuiltinGroup::Set),
    (MEMBER_Q, MemberQ, "member?", "member?[xs;ys]", sig!(arity!(2)), plain(set::member), BuiltinGroup::Set),
    (CART, Cart, "cart", "cart[xs;ys]", sig!(arity!(2)), plain(set::carproduct), BuiltinGroup::Set),
    (IN_Q, InQ, "in?", "in?[x;xs;d?]", sig!(arity!(2, 3)), plain(set::in_), BuiltinGroup::Set, BuiltinDepthSugar::Append { non_depth_argc: 2 }),
    (HAS_Q, HasQ, "has?", "has?[xs;x;d?]", sig!(arity!(2, 3)), plain(set::has), BuiltinGroup::Set, BuiltinDepthSugar::Append { non_depth_argc: 2 }),
    (DISJOINT_Q, DisjointQ, "disjoint?", "disjoint?[xs;ys]", sig!(arity!(2)), plain(set::disjoint), BuiltinGroup::Set),
    (MULTIPLICITY, Multiplicity, "multiplicity", "multiplicity[x;xs]", sig!(arity!(2)), plain(set::multiplicity), BuiltinGroup::Set),

    // Logical ======================================================
    (NOT, Not, "not", "not[xs]", sig!(arity!(1)), plain(logical::not), BuiltinGroup::Logical),
    (XOR, Xor, "xor", "xor[xs;ys+]", sig!(arity!(2..)), plain(logical::xor), BuiltinGroup::Logical),

    (AND, And, "and", "and[xs;ys+]", sig!(arity!(2..)), plain(logical::and), BuiltinGroup::Logical),
    (OR, Or, "or", "or[xs;ys+]", sig!(arity!(2..)), plain(logical::or), BuiltinGroup::Logical),

    (BAND, Band, "band", "band[xs;ys+]", sig!(arity!(2..)), plain(logical::band), BuiltinGroup::Logical),
    (BOR, Bor, "bor", "bor[xs;ys+]", sig!(arity!(2..)), plain(logical::bor), BuiltinGroup::Logical),

    (SHL, Shl, "shl", "shl[xs;shift+]", sig!(arity!(2..)), plain(logical::shl), BuiltinGroup::Logical),
    (SHR, Shr, "shr", "shr[xs;shift+]", sig!(arity!(2..)), plain(logical::shr), BuiltinGroup::Logical),

    // Math =========================================================
    (NEG, Neg, "neg", "neg[xs]", sig!(arity!(1)), plain(math::neg), BuiltinGroup::Math),
    (ABS, Abs, "abs", "abs[xs]", sig!(arity!(1)), plain(math::abs), BuiltinGroup::Math),
    (SGN, Sgn, "sgn", "sgn[xs]", sig!(arity!(1)), plain(math::sgn), BuiltinGroup::Math),
    (SQRT, Sqrt, "sqrt", "sqrt[xs]", sig!(arity!(1)), plain(math::sqrt), BuiltinGroup::Math),
    (EXP, Exp, "exp", "exp[xs]", sig!(arity!(1)), plain(math::exp), BuiltinGroup::Math),
    (LN, Ln, "ln", "ln[xs]", sig!(arity!(1)), plain(math::ln), BuiltinGroup::Math),
    (LOG2, Log2, "log2", "log2[xs]", sig!(arity!(1)), plain(math::log2), BuiltinGroup::Math),
    (LOG10, Log10, "log10", "log10[xs]", sig!(arity!(1)), plain(math::log10), BuiltinGroup::Math),
    (FLOOR, Floor, "floor", "floor[xs;d?]", sig!(arity!(1, 2)), plain(math::floor), BuiltinGroup::Math),
    (CEIL, Ceil, "ceil", "ceil[xs;d?]", sig!(arity!(1, 2)), plain(math::ceil), BuiltinGroup::Math),
    (ROUND, Round, "round", "round[xs;d?]", sig!(arity!(1, 2)), plain(math::round), BuiltinGroup::Math),

    (SIN, Sin, "sin", "sin[xs]", sig!(arity!(1)), plain(math::sin), BuiltinGroup::Math),
    (COS, Cos, "cos", "cos[xs]", sig!(arity!(1)), plain(math::cos), BuiltinGroup::Math),
    (TAN, Tan, "tan", "tan[xs]", sig!(arity!(1)), plain(math::tan), BuiltinGroup::Math),
    (SEC, Sec, "sec", "sec[xs]", sig!(arity!(1)), plain(math::sec), BuiltinGroup::Math),
    (CSC, Csc, "csc", "csc[xs]", sig!(arity!(1)), plain(math::csc), BuiltinGroup::Math),
    (COT, Cot, "cot", "cot[xs]", sig!(arity!(1)), plain(math::cot), BuiltinGroup::Math),
    (ARCSIN, Arcsin, "arcsin", "arcsin[xs]", sig!(arity!(1)), plain(math::arcsin), BuiltinGroup::Math),
    (ARCCOS, Arccos, "arccos", "arccos[xs]", sig!(arity!(1)), plain(math::arccos), BuiltinGroup::Math),
    (ARCTAN, Arctan, "arctan", "arctan[xs]", sig!(arity!(1)), plain(math::arctan), BuiltinGroup::Math),
    (SINH, Sinh, "sinh", "sinh[xs]", sig!(arity!(1)), plain(math::sinh), BuiltinGroup::Math),
    (COSH, Cosh, "cosh", "cosh[xs]", sig!(arity!(1)), plain(math::cosh), BuiltinGroup::Math),
    (TANH, Tanh, "tanh", "tanh[xs]", sig!(arity!(1)), plain(math::tanh), BuiltinGroup::Math),
    (ARCSINH, Arcsinh, "arcsinh", "arcsinh[xs]", sig!(arity!(1)), plain(math::arcsinh), BuiltinGroup::Math),
    (ARCCOSH, Arccosh, "arccosh", "arccosh[xs]", sig!(arity!(1)), plain(math::arccosh), BuiltinGroup::Math),
    (ARCTANH, Arctanh, "arctanh", "arctanh[xs]", sig!(arity!(1)), plain(math::arctanh), BuiltinGroup::Math),
    (LOG, Log, "log", "log[x;a]", sig!(arity!(2)), plain(math::log), BuiltinGroup::Math),
    (ARCTAN2, Arctan2, "arctan2", "arctan2[y;x]", sig!(arity!(2)), plain(math::arctan2), BuiltinGroup::Math),

    (ERF, Erf, "erf", "erf[xs]", sig!(arity!(1)), plain(math::erf), BuiltinGroup::Math),
    (ERFC, Erfc, "erfc", "erfc[xs]", sig!(arity!(1)), plain(math::erfc), BuiltinGroup::Math),
    (GAMMA, Gamma, "gamma", "gamma[xs]", sig!(arity!(1)), plain(math::gamma), BuiltinGroup::Math),
    (LNGAMMA, Lngamma, "lngamma", "lngamma[xs]", sig!(arity!(1)), plain(math::lngamma), BuiltinGroup::Math),
    (SI, Si, "si", "si[xs]", sig!(arity!(1)), plain(math::si), BuiltinGroup::Math),
    (CI, Ci, "ci", "ci[xs]", sig!(arity!(1)), plain(math::ci), BuiltinGroup::Math),
    (EI, Ei, "ei", "ei[xs]", sig!(arity!(1)), plain(math::ei), BuiltinGroup::Math),
    (EN, En, "en", "en[n;xs]", sig!(arity!(2)), plain(math::en), BuiltinGroup::Math),
    (ELLPK, Ellpk, "ellpk", "ellpk[xs]", sig!(arity!(1)), plain(math::ellpk), BuiltinGroup::Math),
    (ELLPE, Ellpe, "ellpe", "ellpe[xs]", sig!(arity!(1)), plain(math::ellpe), BuiltinGroup::Math),
    (ELLIK, Ellik, "ellik", "ellik[phi;m]", sig!(arity!(2)), plain(math::ellik), BuiltinGroup::Math),
    (ELLIE, Ellie, "ellie", "ellie[phi;m]", sig!(arity!(2)), plain(math::ellie), BuiltinGroup::Math),
    (HEAVISIDE, Heaviside, "heaviside", "heaviside[xs]", sig!(arity!(1)), plain(math::heaviside), BuiltinGroup::Math),
    (DELTA, Delta, "delta", "delta[xs]", sig!(arity!(1)), plain(math::delta), BuiltinGroup::Math),

    // Rand
    (RAND, Rand, "rand", "rand[]; rand[upper]; rand[lower;upper]", sig!(arity!(0, 1, 2)), plain(math::rand), BuiltinGroup::Rand),

    // Complex
    (COMPLEX, Complex, "complex", "complex[re;im]", sig!(arity!(2)), plain(complex::complex), BuiltinGroup::Complex),
    (RE, Re, "re", "re[x]", sig!(arity!(1)), plain(complex::real), BuiltinGroup::Complex),
    (IM, Im, "im", "im[x]", sig!(arity!(1)), plain(complex::imag), BuiltinGroup::Complex),
    (CONJ, Conj, "conj", "conj[x]", sig!(arity!(1)), plain(complex::conj), BuiltinGroup::Complex),

    // Fraction
    (FRACTION, Fraction, "fraction", "fraction[xs;lim?]", sig!(arity!(1, 2)), plain(fraction::fraction), BuiltinGroup::Fraction),
    (FRACTIONL, Fractionl, "fractionl", "fractionl[xs]", sig!(arity!(1)), plain(fraction::fractionl), BuiltinGroup::Fraction),

    // CAS
    (EQ, Eq, "eq", "eq[lhs;rhs]", sig!(arity!(2)), plain(cas::eq), BuiltinGroup::Cas),
    (SIMPLIFY, Simplify, "simplify", "simplify[expr]", sig!(arity!(1)), plain(cas::simplify), BuiltinGroup::Cas),
    (REWRITE, Rewrite, "rewrite", "rewrite[expr]", sig!(arity!(1)), plain(cas::rewrite), BuiltinGroup::Cas),
    (NUMERIC, Numeric, "numeric", "numeric[expr], numeric[expr;`name:val...]", sig!(arity!(1), defer), plain(cas::numeric), BuiltinGroup::Cas),
    (DIFF, Diff, "diff", "diff[expr;var?]", sig!(arity!(1, 2)), plain(cas::diff), BuiltinGroup::Cas),
    (D, D, "D", "D[expr;var?]", sig!(arity!(1, 2), alias Diff), plain(cas::diff), BuiltinGroup::Cas), // alias of diff
    (SUBSTITUTE, Substitute, "substitute", "substitute[expr;eqs], substitute[expr;var;val], substitute[expr;`name:val...]", sig!(arity!(1, 2, 3), defer), plain(cas::substitute), BuiltinGroup::Cas),
    (EXPAND, Expand, "expand", "expand[expr]", sig!(arity!(1)), plain(cas::expand), BuiltinGroup::Cas),
    (FACTOR_COMMON, FactorCommon, "factor_common", "factor_common[expr]", sig!(arity!(1)), plain(cas::factor_common), BuiltinGroup::Cas),
    (FACTOR, Factor, "factor", "factor[expr], factor[expr;var], factor[expr;1], factor[expr;1;var]", sig!(arity!(1, 2, 3)), plain(cas::factor_poly), BuiltinGroup::Cas),
    (INTEGRATE, Integrate, "integrate", "integrate[expr], integrate[expr;var], integrate[expr;var;lower;upper]", sig!(arity!(1, 2, 4)), plain(cas::integrate), BuiltinGroup::Cas),
    (I, I, "I", "I[expr], I[expr;var], I[expr;var;lower;upper]", sig!(arity!(1, 2, 4), alias Integrate), plain(cas::integrate), BuiltinGroup::Cas), // alias of integrate
    (LIMIT, Limit, "limit", "limit[expr;point;`d], limit[expr;var;point;`d]", sig!(arity!(2..), named LIMIT_NAMED_ARGS), plain(cas::limit), BuiltinGroup::Cas),
    (SOLVE, Solve, "solve", "solve[expr], solve[expr;var], solve[eq;var]", sig!(arity!(1, 2)), plain(cas::solve), BuiltinGroup::Cas),
    (SOLVE_SYSTEM, SolveSystem, "solve_system", "solve_system[eqs], solve_system[eqs;vars]", sig!(arity!(1, 2)), plain(cas::solve_system), BuiltinGroup::Cas),
    (BRENT, Brent, "brent", "brent[expr;a;b], brent[expr;a;b;tol], brent[expr;a;b;tol;max_iter], brent[eq;a;b]", sig!(arity!(3, 4, 5)), plain(cas::brent), BuiltinGroup::Cas),
    (NEWTON, Newton, "newton", "newton[expr;x0], newton[expr;x0;tol], newton[expr;x0;tol;max_iter], newton[eq;x0]", sig!(arity!(2, 3, 4)), plain(cas::newton), BuiltinGroup::Cas),

    // String =========================================================
    (STR, Str, "str", "str[x]", sig!(arity!(1)), plain(string::to_str), BuiltinGroup::Str),
    (GRAPHEMES, Graphemes, "graphemes", "graphemes[s]", sig!(arity!(1)), plain(string::graphemes), BuiltinGroup::Str),
    (WS_Q, WsQ, "ws?", "ws?[c]", sig!(arity!(1)), plain(string::is_whitespace), BuiltinGroup::Str),
    (WORDS, Words, "words", "words[s]", sig!(arity!(1)), plain(string::words), BuiltinGroup::Str),
    (TRIM, Trim, "trim", "trim[s]", sig!(arity!(1)), plain(string::trim), BuiltinGroup::Str),
    (L_TRIM, LTrim, "ltrim", "ltrim[s]", sig!(arity!(1)), plain(string::trim_left), BuiltinGroup::Str),
    (R_TRIM, RTrim, "rtrim", "rtrim[s]", sig!(arity!(1)), plain(string::trim_right), BuiltinGroup::Str),

    // Type =========================================================
    (TYPE, Type, "type", "type[x]", sig!(arity!(1)), plain(wqtype::type_of), BuiltinGroup::Type),
    (TAG, Tag, "tag", "tag[x]", sig!(arity!(1)), plain(wqtype::to_tag), BuiltinGroup::Type),
    (BOOL, Bool, "bool", "bool[x]", sig!(arity!(1)), plain(wqtype::to_bool), BuiltinGroup::Type),
    (CHAR, Char, "char", "char[x]", sig!(arity!(1)), plain(wqtype::to_char), BuiltinGroup::Type),
    (ATOM_Q, AtomQ, "atom?", "atom?[x]", sig!(arity!(1)), plain(wqtype::is_atom), BuiltinGroup::Type),
    (UNIT_Q, UnitQ, "unit?", "unit?[x]", sig!(arity!(1)), plain(wqtype::is_unit), BuiltinGroup::Type),
    (U, U, "U", "U[x]", sig!(arity!(1), alias UnitQ), plain(wqtype::is_unit), BuiltinGroup::Type), // alias of unit?
    (LIST, List, "list", "list[x]", sig!(arity!(1)), plain(wqtype::to_list), BuiltinGroup::Type),
    (DICT, Dict, "dict", "dict[x]", sig!(arity!(1)), plain(wqtype::to_dict), BuiltinGroup::Type),

    // Visualization =========================================================
    (SHOWTABLE, Showtable, "showtable", "showtable[table;`cols;`limit;`width;`style;`missing]", sig!(arity!(1), named SHOWTABLE_NAMED_ARGS), plain(viz::show_table), BuiltinGroup::Viz),
    (ASCIIPLOT, Asciiplot, "asciiplot",
        concat!("asciiplot[data+;`size;`width;`height;`xlim;`ylim;",
            "`x;`y;`symbols;`labels;`mode;`axes;`color;`grid;",
            "`samples;`theme;`complex;`ascii;",
            "`ticklabels;`title;`xlabel;`ylabel;`caption]"), sig!(arity!(1..), named ASCIIPLOT_NAMED_ARGS), with_context(viz::asciiplot), BuiltinGroup::Viz),

    // Intrinsic ====================================================
    (FMT, Fmt, "fmt", "fmt[template;v*]", sig!(arity!(1..)), plain(string::fmt), BuiltinGroup::Intrinsic),

    (OP_ADD, OpAdd, "+", "+[xs;ys+]", sig!(arity!(2..)), plain(op::op_add), BuiltinGroup::Intrinsic),
    (OP_SUB, OpSub, "-", "-[x], -[xs;ys+]", sig!(arity!(1..)), plain(op::op_sub), BuiltinGroup::Intrinsic),
    (OP_MUL, OpMul, "*", "*[xs;ys+]", sig!(arity!(2..)), plain(op::op_mul), BuiltinGroup::Intrinsic),
    (OP_DIV, OpDiv, "/", "/[xs;ys+]", sig!(arity!(2..)), plain(op::op_div), BuiltinGroup::Intrinsic),
    (OP_DIV_DOT, OpDivDot, "/.", "/.[xs;ys+]", sig!(arity!(2..)), plain(op::op_divdot), BuiltinGroup::Intrinsic),
    (OP_MOD, OpMod, "%", "%[xs;ys+]", sig!(arity!(2..)), plain(op::op_mod), BuiltinGroup::Intrinsic),
    (OP_FLOORDIV, OpFloorDiv, "/%", "/%[xs;ys+]", sig!(arity!(2..)), plain(op::op_floordiv), BuiltinGroup::Intrinsic),
    (OP_POWER, OpPower, "^", "^[xs;ys+]", sig!(arity!(2..)), plain(op::op_power), BuiltinGroup::Intrinsic),
    (OP_POWER_DOT, OpPowerDot, "^.", "^.[xs;ys+]", sig!(arity!(2..)), plain(op::op_power_dot), BuiltinGroup::Intrinsic),
    (OP_MATMUL, OpMatmul, "**", "**[xs;ys+]", sig!(arity!(2..)), plain(op::op_matmul), BuiltinGroup::Intrinsic),

    (OP_EQUAL, OpEqual, "=", "=[xs;ys+]", sig!(arity!(2..)), plain(op::op_equal), BuiltinGroup::Intrinsic),
    (OP_EQUAL_DOT, OpEqualDot, "=.", "=.[xs;ys+]", sig!(arity!(2..)), plain(op::op_equal_dot), BuiltinGroup::Intrinsic),
    (OP_TILDE, OpTilde, "~", "~[x], ~[xs;ys+]", sig!(arity!(1..)), plain(op::op_tilde), BuiltinGroup::Intrinsic),
    (OP_TILDE_DOT, OpTildeDot, "~.", "~.[xs;ys+]", sig!(arity!(2..)), plain(op::op_tilde_dot), BuiltinGroup::Intrinsic),
    (OP_LT, OpLt, "<", "<[xs;ys+]", sig!(arity!(2..)), plain(op::op_lt), BuiltinGroup::Intrinsic),
    (OP_LTE, OpLte, "<=", "<=[xs;ys+]", sig!(arity!(2..)), plain(op::op_lte), BuiltinGroup::Intrinsic),
    (OP_GT, OpGt, ">", ">[xs;ys+]", sig!(arity!(2..)), plain(op::op_gt), BuiltinGroup::Intrinsic),
    (OP_GTE, OpGte, ">=", ">=[xs;ys+]", sig!(arity!(2..)), plain(op::op_gte), BuiltinGroup::Intrinsic),

    (OP_CAT, OpCat, ",", ",[xs;ys+]", sig!(arity!(2..)), plain(op::op_cat), BuiltinGroup::Intrinsic),
    (OP_SHARP, OpSharp, "#", "#[x]", sig!(arity!(1)), plain(op::op_sharp), BuiltinGroup::Intrinsic),

    (OP_SHL, OpShl, "<<", "<<[xs;ys+]", sig!(arity!(2..)), plain(op::op_shl), BuiltinGroup::Intrinsic),
    (OP_SHR, OpShr, ">>", ">>[xs;ys+]", sig!(arity!(2..)), plain(op::op_shr), BuiltinGroup::Intrinsic),

}

const ECHO_NAMED_ARGS: &[&str] = &["sep"];
const MAXSPLIT_NAMED_ARGS: &[&str] = &["m"];
const LIMIT_NAMED_ARGS: &[&str] = &["d"];
const SHOWTABLE_NAMED_ARGS: &[&str] = &["cols", "limit", "width", "style", "missing"];
const ASCIIPLOT_NAMED_ARGS: &[&str] = &[
    "size",
    "width",
    "height",
    "xlim",
    "ylim",
    "x",
    "y",
    "symbols",
    "labels",
    "mode",
    "axes",
    "color",
    "grid",
    "samples",
    "theme",
    "complex",
    "ascii",
    "title",
    "xlabel",
    "ylabel",
    "caption",
    "ticklabels",
];
#[cfg(not(target_arch = "wasm32"))]
const EXEC_NAMED_ARGS: &[&str] = &["stdin", "cwd", "env", "timeout", "check"];
#[cfg(not(target_arch = "wasm32"))]
const OPEN_NAMED_ARGS: &[&str] = &["r", "w", "a", "t", "c", "cn"];

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct BuiltinCallCheck {
    src: BuiltinEnum,
    arity: BuiltinCallArity,
    named: BuiltinNamedArgs,
}

impl BuiltinCallCheck {
    fn from_signature(builtin: BuiltinEnum, signature: BuiltinSignature) -> Option<Self> {
        if signature.validation == BuiltinValidation::Defer {
            return None;
        }

        Some(Self {
            src: signature.source.unwrap_or(builtin),
            arity: signature.arity,
            named: signature.named,
        })
    }

    fn validate(self, args: &BuiltinFnArgs) -> WqResult<()> {
        self.arity.validate(self.src, args)?;
        match self.named {
            BuiltinNamedArgs::Deny => deny_named_args(args, self.src),
            BuiltinNamedArgs::Allow(allowed) => check_named_args(args, self.src, allowed),
        }
    }
}

impl Builtins {
    fn build_call_checks() -> Vec<Option<BuiltinCallCheck>> {
        Self::ENUMS
            .iter()
            .copied()
            .zip(Self::SIGNATURES.iter().copied())
            .map(|(builtin, signature)| BuiltinCallCheck::from_signature(builtin, signature))
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

/// Like check_arity but accepts arbitrary named args.
#[inline]
pub(super) fn check_arity_any_named(
    builtin: BuiltinEnum,
    arity: impl AsRef<[usize]>,
    args: &BuiltinFnArgs,
) -> WqResult<()> {
    if args.runtime_validated() {
        return Ok(());
    }

    check_arity_inner(builtin, arity, args)
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
    fn builtin_signature_table_stays_in_lockstep() {
        let builtins = Builtins::new();

        assert_eq!(Builtins::ENUMS.len(), Builtins::NAMES.len());
        assert_eq!(Builtins::ENUMS.len(), Builtins::USAGES.len());
        assert_eq!(Builtins::ENUMS.len(), Builtins::SIGNATURES.len());
        assert_eq!(builtins.functions.len(), Builtins::SIGNATURES.len());
    }

    #[test]
    fn builtin_signature_displays_match_legacy_arity_strings() {
        let expected = [
            (BuiltinEnum::Bfn, "0"),
            (BuiltinEnum::Chr, "1"),
            (BuiltinEnum::Ord, "1"),
            (BuiltinEnum::Int, "1 2"),
            (BuiltinEnum::Float, "1"),
            (BuiltinEnum::Bin, "1 2"),
            (BuiltinEnum::Oct, "1 2"),
            (BuiltinEnum::Hex, "1 2"),
            (BuiltinEnum::Hash, "1"),
            (BuiltinEnum::Raise, "0 1"),
            (BuiltinEnum::Echo, "0.."),
            (BuiltinEnum::E, "0.."),
            (BuiltinEnum::Print, "0.."),
            (BuiltinEnum::Input, "0 1"),
            #[cfg(not(target_arch = "wasm32"))]
            (BuiltinEnum::Exec, "1.."),
            (BuiltinEnum::Decode, "2 3"),
            (BuiltinEnum::Encode, "2 3"),
            (BuiltinEnum::ValidBytes, "1"),
            #[cfg(not(target_arch = "wasm32"))]
            (BuiltinEnum::Open, "1"),
            #[cfg(not(target_arch = "wasm32"))]
            (BuiltinEnum::FexistsQ, "1"),
            #[cfg(not(target_arch = "wasm32"))]
            (BuiltinEnum::Mkdir, "1"),
            #[cfg(not(target_arch = "wasm32"))]
            (BuiltinEnum::Fsize, "1"),
            #[cfg(not(target_arch = "wasm32"))]
            (BuiltinEnum::Fwrite, "2"),
            #[cfg(not(target_arch = "wasm32"))]
            (BuiltinEnum::Fwritet, "2"),
            #[cfg(not(target_arch = "wasm32"))]
            (BuiltinEnum::Fread, "1 2"),
            #[cfg(not(target_arch = "wasm32"))]
            (BuiltinEnum::Freadt, "1 2"),
            #[cfg(not(target_arch = "wasm32"))]
            (BuiltinEnum::Freadtln, "1"),
            #[cfg(not(target_arch = "wasm32"))]
            (BuiltinEnum::Freadtlns, "1"),
            #[cfg(not(target_arch = "wasm32"))]
            (BuiltinEnum::Fseek, "2 3"),
            #[cfg(not(target_arch = "wasm32"))]
            (BuiltinEnum::Ftell, "1"),
            #[cfg(not(target_arch = "wasm32"))]
            (BuiltinEnum::Fclose, "1"),
            (BuiltinEnum::Len, "1"),
            (BuiltinEnum::StrongCount, "1"),
            (BuiltinEnum::Shape, "1"),
            (BuiltinEnum::Depth, "1"),
            (BuiltinEnum::UniformQ, "1"),
            (BuiltinEnum::Sum, "0.."),
            (BuiltinEnum::Product, "0.."),
            (BuiltinEnum::Min, "1.."),
            (BuiltinEnum::Max, "1.."),
            (BuiltinEnum::Flatten, "1"),
            (BuiltinEnum::Reverse, "1"),
            (BuiltinEnum::V, "1"),
            (BuiltinEnum::Sort, "1"),
            (BuiltinEnum::Split, "1 2"),
            (BuiltinEnum::Find, "2 3 4"),
            (BuiltinEnum::RFind, "2 3 4"),
            (BuiltinEnum::Zip, "2 3"),
            (BuiltinEnum::Alloc, "1 2"),
            (BuiltinEnum::Til, "1"),
            (BuiltinEnum::Iota, "1"),
            (BuiltinEnum::Range, "2 3"),
            (BuiltinEnum::Reshape, "2"),
            (BuiltinEnum::R, "2"),
            (BuiltinEnum::Transpose, "1 2"),
            (BuiltinEnum::TP, "1 2"),
            (BuiltinEnum::Repeat, "2"),
            (BuiltinEnum::Where, "1"),
            (BuiltinEnum::Z, "1"),
            (BuiltinEnum::Apply, "2"),
            (BuiltinEnum::Map, "2 3"),
            (BuiltinEnum::M, "2 3"),
            (BuiltinEnum::Fold, "2 3"),
            (BuiltinEnum::Reduce, "2 3"),
            (BuiltinEnum::Scan, "2 3"),
            (BuiltinEnum::RScan, "2 3"),
            (BuiltinEnum::Any, "2 3"),
            (BuiltinEnum::All, "2 3"),
            (BuiltinEnum::Filter, "2"),
            (BuiltinEnum::ZipW, "3 4"),
            (BuiltinEnum::SplitW, "2"),
            (BuiltinEnum::FindW, "2 3 4"),
            (BuiltinEnum::RFindW, "2 3 4"),
            (BuiltinEnum::Keys, "1"),
            (BuiltinEnum::IdxToKey, "2"),
            (BuiltinEnum::KeyToIdx, "2"),
            (BuiltinEnum::Unique, "1"),
            (BuiltinEnum::Counts, "1"),
            (BuiltinEnum::Union, "2"),
            (BuiltinEnum::Intersect, "2"),
            (BuiltinEnum::Without, "2"),
            (BuiltinEnum::Symdiff, "2"),
            (BuiltinEnum::SubQ, "2"),
            (BuiltinEnum::SuperQ, "2"),
            (BuiltinEnum::PSubQ, "2"),
            (BuiltinEnum::PSuperQ, "2"),
            (BuiltinEnum::MemberQ, "2"),
            (BuiltinEnum::Cart, "2"),
            (BuiltinEnum::InQ, "2 3"),
            (BuiltinEnum::HasQ, "2 3"),
            (BuiltinEnum::DisjointQ, "2"),
            (BuiltinEnum::Multiplicity, "2"),
            (BuiltinEnum::Not, "1"),
            (BuiltinEnum::Xor, "2.."),
            (BuiltinEnum::And, "2.."),
            (BuiltinEnum::Or, "2.."),
            (BuiltinEnum::Band, "2.."),
            (BuiltinEnum::Bor, "2.."),
            (BuiltinEnum::Shl, "2.."),
            (BuiltinEnum::Shr, "2.."),
            (BuiltinEnum::Neg, "1"),
            (BuiltinEnum::Abs, "1"),
            (BuiltinEnum::Sgn, "1"),
            (BuiltinEnum::Sqrt, "1"),
            (BuiltinEnum::Exp, "1"),
            (BuiltinEnum::Ln, "1"),
            (BuiltinEnum::Log2, "1"),
            (BuiltinEnum::Log10, "1"),
            (BuiltinEnum::Floor, "1 2"),
            (BuiltinEnum::Ceil, "1 2"),
            (BuiltinEnum::Round, "1 2"),
            (BuiltinEnum::Sin, "1"),
            (BuiltinEnum::Cos, "1"),
            (BuiltinEnum::Tan, "1"),
            (BuiltinEnum::Sec, "1"),
            (BuiltinEnum::Csc, "1"),
            (BuiltinEnum::Cot, "1"),
            (BuiltinEnum::Arcsin, "1"),
            (BuiltinEnum::Arccos, "1"),
            (BuiltinEnum::Arctan, "1"),
            (BuiltinEnum::Sinh, "1"),
            (BuiltinEnum::Cosh, "1"),
            (BuiltinEnum::Tanh, "1"),
            (BuiltinEnum::Arcsinh, "1"),
            (BuiltinEnum::Arccosh, "1"),
            (BuiltinEnum::Arctanh, "1"),
            (BuiltinEnum::Log, "2"),
            (BuiltinEnum::Arctan2, "2"),
            (BuiltinEnum::Erf, "1"),
            (BuiltinEnum::Erfc, "1"),
            (BuiltinEnum::Gamma, "1"),
            (BuiltinEnum::Lngamma, "1"),
            (BuiltinEnum::Si, "1"),
            (BuiltinEnum::Ci, "1"),
            (BuiltinEnum::Ei, "1"),
            (BuiltinEnum::En, "2"),
            (BuiltinEnum::Ellpk, "1"),
            (BuiltinEnum::Ellpe, "1"),
            (BuiltinEnum::Ellik, "2"),
            (BuiltinEnum::Ellie, "2"),
            (BuiltinEnum::Heaviside, "1"),
            (BuiltinEnum::Delta, "1"),
            (BuiltinEnum::Rand, "0 1 2"),
            (BuiltinEnum::Complex, "2"),
            (BuiltinEnum::Re, "1"),
            (BuiltinEnum::Im, "1"),
            (BuiltinEnum::Conj, "1"),
            (BuiltinEnum::Fraction, "1 2"),
            (BuiltinEnum::Fractionl, "1"),
            (BuiltinEnum::Eq, "2"),
            (BuiltinEnum::Simplify, "1"),
            (BuiltinEnum::Rewrite, "1"),
            (BuiltinEnum::Numeric, "1"),
            (BuiltinEnum::Diff, "1 2"),
            (BuiltinEnum::D, "1 2"),
            (BuiltinEnum::Substitute, "1 2 3"),
            (BuiltinEnum::Expand, "1"),
            (BuiltinEnum::FactorCommon, "1"),
            (BuiltinEnum::Factor, "1 2 3"),
            (BuiltinEnum::Integrate, "1 2 4"),
            (BuiltinEnum::I, "1 2 4"),
            (BuiltinEnum::Limit, "2.."),
            (BuiltinEnum::Solve, "1 2"),
            (BuiltinEnum::SolveSystem, "1 2"),
            (BuiltinEnum::Brent, "3 4 5"),
            (BuiltinEnum::Newton, "2 3 4"),
            (BuiltinEnum::Str, "1"),
            (BuiltinEnum::Graphemes, "1"),
            (BuiltinEnum::WsQ, "1"),
            (BuiltinEnum::Words, "1"),
            (BuiltinEnum::Trim, "1"),
            (BuiltinEnum::LTrim, "1"),
            (BuiltinEnum::RTrim, "1"),
            (BuiltinEnum::Type, "1"),
            (BuiltinEnum::Tag, "1"),
            (BuiltinEnum::Bool, "1"),
            (BuiltinEnum::Char, "1"),
            (BuiltinEnum::AtomQ, "1"),
            (BuiltinEnum::UnitQ, "1"),
            (BuiltinEnum::U, "1"),
            (BuiltinEnum::List, "1"),
            (BuiltinEnum::Dict, "1"),
            (BuiltinEnum::Showtable, "1"),
            (BuiltinEnum::Asciiplot, "1.."),
            (BuiltinEnum::Fmt, "1.."),
            (BuiltinEnum::OpAdd, "2.."),
            (BuiltinEnum::OpSub, "1.."),
            (BuiltinEnum::OpMul, "2.."),
            (BuiltinEnum::OpDiv, "2.."),
            (BuiltinEnum::OpDivDot, "2.."),
            (BuiltinEnum::OpMod, "2.."),
            (BuiltinEnum::OpFloorDiv, "2.."),
            (BuiltinEnum::OpPower, "2.."),
            (BuiltinEnum::OpPowerDot, "2.."),
            (BuiltinEnum::OpMatmul, "2.."),
            (BuiltinEnum::OpEqual, "2.."),
            (BuiltinEnum::OpEqualDot, "2.."),
            (BuiltinEnum::OpTilde, "1.."),
            (BuiltinEnum::OpTildeDot, "2.."),
            (BuiltinEnum::OpLt, "2.."),
            (BuiltinEnum::OpLte, "2.."),
            (BuiltinEnum::OpGt, "2.."),
            (BuiltinEnum::OpGte, "2.."),
            (BuiltinEnum::OpCat, "2.."),
            (BuiltinEnum::OpSharp, "1"),
            (BuiltinEnum::OpShl, "2.."),
            (BuiltinEnum::OpShr, "2.."),
        ];

        assert_eq!(expected.len(), Builtins::ENUMS.len());
        for (builtin, expected_text) in expected {
            assert_eq!(builtin.arity().to_string(), expected_text, "{builtin}");
            assert_eq!(
                Builtins::arity_from_id(builtin.id())
                    .expect("builtin arity from id")
                    .to_string(),
                expected_text,
                "{builtin}",
            );
        }
    }

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
                    Builtins::FIND,
                    &BuiltinFnArgs::from(vec![Value::Int(1), Value::Int(2)]),
                )
                .expect("find runtime validation should succeed")
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
    fn runtime_call_checks_validate_promoted_atleast_builtins() {
        let builtins = Builtins::new();

        assert!(
            builtins
                .validate_runtime_call_args(Builtins::PRINT, &BuiltinFnArgs::new())
                .expect("print with zero args should pass runtime validation")
        );
        assert!(
            builtins
                .validate_runtime_call_args(Builtins::SUM, &BuiltinFnArgs::new())
                .expect("sum with zero args should pass runtime validation")
        );
        assert!(
            builtins
                .validate_runtime_call_args(Builtins::PRODUCT, &BuiltinFnArgs::new())
                .expect("product with zero args should pass runtime validation")
        );

        let cases = [
            (
                Builtins::MIN,
                BuiltinFnArgs::new(),
                "expected 1 or more args, got 0",
            ),
            (
                Builtins::LIMIT,
                BuiltinFnArgs::from(Value::Int(1)),
                "expected 2 or more args, got 1",
            ),
            (
                Builtins::FMT,
                BuiltinFnArgs::new(),
                "expected 1 or more args, got 0",
            ),
            (
                Builtins::OP_ADD,
                BuiltinFnArgs::from(Value::Int(1)),
                "expected 2 or more args, got 1",
            ),
            (
                Builtins::OP_SUB,
                BuiltinFnArgs::new(),
                "expected 1 or more args, got 0",
            ),
            (
                Builtins::AND,
                BuiltinFnArgs::from(Value::Bool(true)),
                "expected 2 or more args, got 1",
            ),
        ];
        for (id, args, expected_msg) in cases {
            let err = match builtins.validate_runtime_call_args(id, &args) {
                Ok(validated) => panic!(
                    "{} should reject too few args, got Ok({validated})",
                    Builtins::name_from_id(id).unwrap_or("<unknown builtin>")
                ),
                Err(err) => err,
            };
            assert_eq!(err.msg.as_deref(), Some(expected_msg));
        }

        assert!(
            builtins
                .validate_runtime_call_args(
                    Builtins::OP_ADD,
                    &BuiltinFnArgs::from(vec![Value::Int(1), Value::Int(2)]),
                )
                .expect("operator with enough args should pass runtime validation")
        );
    }

    #[test]
    fn runtime_call_checks_cover_all_registered_builtins() {
        let builtins = Builtins::new();

        let deferred: Vec<_> = Builtins::ENUMS
            .iter()
            .zip(Builtins::SIGNATURES.iter())
            .filter_map(|(builtin, signature)| {
                (signature.validation == BuiltinValidation::Defer).then_some(*builtin)
            })
            .collect();
        assert_eq!(
            deferred,
            vec![BuiltinEnum::Numeric, BuiltinEnum::Substitute]
        );
        assert!(
            builtins
                .call_checks
                .iter()
                .zip(Builtins::SIGNATURES.iter())
                .all(|(check, signature)| {
                    check.is_some() == (signature.validation == BuiltinValidation::Fast)
                })
        );
        assert_eq!(
            Builtins::SIGNATURES
                .iter()
                .filter(|signature| signature.validation == BuiltinValidation::Fast)
                .count(),
            builtins
                .call_checks
                .iter()
                .filter(|check| check.is_some())
                .count()
        );
    }

    #[test]
    fn runtime_call_checks_use_canonical_alias_sources() {
        let builtins = Builtins::new();

        let err = builtins
            .validate_runtime_call_args(Builtins::V, &BuiltinFnArgs::new())
            .expect_err("V with zero args should fail runtime validation");
        assert_eq!(err.src.as_deref(), Some("bfn 'reverse'"));

        let err = builtins
            .validate_runtime_call_args(Builtins::M, &BuiltinFnArgs::new())
            .expect_err("M with zero args should fail runtime validation");
        assert_eq!(err.src.as_deref(), Some("bfn 'map'"));
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

        let print_bad_args = BuiltinFnArgs::with_named(
            SmallVec::new(),
            vec![(Arc::<str>::from("bad"), Value::Int(1))],
        );
        let err = builtins
            .validate_runtime_call_args(Builtins::PRINT, &print_bad_args)
            .expect_err("print should reject named args");
        assert_eq!(err.msg.as_deref(), Some("unexpected named argument 'bad'"));

        let sum_bad_args = BuiltinFnArgs::with_named(
            SmallVec::new(),
            vec![(Arc::<str>::from("bad"), Value::Int(1))],
        );
        let err = builtins
            .validate_runtime_call_args(Builtins::SUM, &sum_bad_args)
            .expect_err("sum should reject named args");
        assert_eq!(err.msg.as_deref(), Some("unexpected named argument 'bad'"));

        let asciiplot_args = BuiltinFnArgs::with_named(
            SmallVec::from_vec(vec![Value::IntList(Arc::new(vec![1, 2, 3]))]),
            vec![(Arc::<str>::from("width"), Value::Int(40))],
        );
        assert!(
            builtins
                .validate_runtime_call_args(Builtins::ASCIIPLOT, &asciiplot_args)
                .expect("asciiplot named runtime validation should succeed")
        );

        let asciiplot_bad_args = BuiltinFnArgs::with_named(
            SmallVec::from_vec(vec![Value::IntList(Arc::new(vec![1, 2, 3]))]),
            vec![(Arc::<str>::from("bad"), Value::Int(1))],
        );
        let err = builtins
            .validate_runtime_call_args(Builtins::ASCIIPLOT, &asciiplot_bad_args)
            .expect_err("asciiplot should reject unknown named args");
        assert_eq!(err.msg.as_deref(), Some("unknown named argument 'bad'"));

        #[cfg(not(target_arch = "wasm32"))]
        {
            let exec_args = BuiltinFnArgs::with_named(
                SmallVec::from_vec(vec![into_wq_string("cat")]),
                vec![(Arc::<str>::from("stdin"), into_wq_string("input"))],
            );
            assert!(
                builtins
                    .validate_runtime_call_args(Builtins::EXEC, &exec_args)
                    .expect("exec named runtime validation should succeed")
            );
        }
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
