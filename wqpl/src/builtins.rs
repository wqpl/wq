mod cas;
mod cli;
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
mod random;
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
pub enum BuiltinCategory {
    Core,
    Exec,
    Encoding,
    FileIO,
    Meta,
    List,
    ListGen,
    Mat,
    HigherOrder,
    Dict,
    Set,
    Logical,
    Math,
    Rand,
    Complex,
    Fraction,
    Cas,
    Str,
    Type,
    Viz,
}

impl BuiltinCategory {
    pub const ALL: &'static [Self] = &[
        Self::Core,
        #[cfg(not(target_arch = "wasm32"))]
        Self::Exec,
        Self::Encoding,
        #[cfg(not(target_arch = "wasm32"))]
        Self::FileIO,
        Self::Meta,
        Self::List,
        Self::ListGen,
        Self::Mat,
        Self::HigherOrder,
        Self::Dict,
        Self::Set,
        Self::Logical,
        Self::Math,
        Self::Rand,
        Self::Complex,
        Self::Fraction,
        Self::Cas,
        Self::Str,
        Self::Type,
        Self::Viz,
    ];

    pub fn name(self) -> &'static str {
        match self {
            BuiltinCategory::Cas => "CAS",
            BuiltinCategory::Complex => "Complex",
            BuiltinCategory::Core => "Core",
            BuiltinCategory::Exec => "Exec",
            BuiltinCategory::Dict => "Dict",
            BuiltinCategory::Encoding => "Encoding",
            BuiltinCategory::Fraction => "Fraction",
            BuiltinCategory::HigherOrder => "Higher-Order",
            BuiltinCategory::FileIO => "File IO",
            BuiltinCategory::List => "List",
            BuiltinCategory::ListGen => "List Generation",
            BuiltinCategory::Logical => "Logical",
            BuiltinCategory::Mat => "Matrix",
            BuiltinCategory::Meta => "Meta",
            BuiltinCategory::Math => "Math",
            BuiltinCategory::Rand => "Random",
            BuiltinCategory::Set => "Set",
            BuiltinCategory::Str => "String",
            BuiltinCategory::Viz => "Visualization",
            BuiltinCategory::Type => "Type",
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct BuiltinPolicy {
    minimal: bool,
    pure: bool,
    constrained: bool,
    const_foldable: bool,
}

impl BuiltinPolicy {
    const REQUIRED: Self = Self::new(true, true, true, true);
    const REQUIRED_CONTEXTUAL: Self = Self::new(true, true, true, false);
    const PURE: Self = Self::new(false, true, true, true);
    const PURE_CONTEXTUAL: Self = Self::new(false, true, true, false);
    const CONSTRAINED_EFFECT: Self = Self::new(false, false, true, false);
    #[cfg(not(target_arch = "wasm32"))]
    const UNCONSTRAINED_EFFECT: Self = Self::new(false, false, false, false);

    const fn new(minimal: bool, pure: bool, constrained: bool, const_foldable: bool) -> Self {
        assert!(!minimal || pure, "minimal builtins must be pure");
        assert!(!pure || constrained, "pure builtins must be constrained");
        assert!(
            !const_foldable || pure,
            "constant-foldable builtins must be pure"
        );

        Self {
            minimal,
            pure,
            constrained,
            const_foldable,
        }
    }

    pub const fn is_enabled_in(self, preset: BuiltinPreset) -> bool {
        match preset {
            BuiltinPreset::All => true,
            BuiltinPreset::Pure => self.pure,
            BuiltinPreset::Minimal => self.minimal,
            BuiltinPreset::Constrained => self.constrained,
        }
    }

    pub const fn is_const_foldable(self) -> bool {
        self.const_foldable
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct BuiltinMetadata {
    pub category: BuiltinCategory,
    pub policy: BuiltinPolicy,
}

impl BuiltinMetadata {
    const fn new(category: BuiltinCategory, policy: BuiltinPolicy) -> Self {
        Self { category, policy }
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
/// Positional args: `SmallVec<[Value; 4]>`, zero heap for <=4 args.
/// Named args: `Option<Vec<(Arc<str>, Value)>>`, heap-allocated, None when
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
    pub(crate) fn into_parts(self) -> (Sv4, Vec<(Arc<str>, Value)>) {
        (self.pos, self.named.unwrap_or_default())
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
    fn draw_default_random(&mut self, args: &[Value]) -> WqResult<Value>;
    fn list_enabled_builtins(&self) -> Vec<String>;
    fn argv(&self) -> &[String];
    fn request_halt(&mut self, status: i32);
    fn requires_callback_frames(&self) -> bool {
        false
    }
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
            Self::Exact { mask } if u32::try_from(n).is_ok_and(|n| n < u128::BITS) => {
                mask & (1u128 << n) != 0
            }
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
        debug_assert_eq!(
            BUILTIN_METADATA.len(),
            total,
            "builtin metadata out of sync"
        );
        self.enabled = vec![false; total];
        for (idx, metadata) in BUILTIN_METADATA.iter().enumerate() {
            if metadata.policy.is_enabled_in(preset) {
                self.enabled[idx] = true;
            }
        }
    }

    pub fn is_enabled_id(&self, id: u16) -> bool {
        self.enabled.get(usize::from(id)).copied().unwrap_or(false)
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
        let previous = self.name_to_id.insert(name.to_string(), id);
        assert!(previous.is_none(), "duplicate builtin name '{name}'");
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

    pub fn list_functions_by_category(&self) -> Vec<(&'static str, Vec<String>)> {
        use std::collections::BTreeMap;
        let mut categorized: BTreeMap<BuiltinCategory, Vec<String>> = BTreeMap::new();
        for (name, &id) in self.name_to_id.iter() {
            if self.enabled.get(id).copied().unwrap_or(false) {
                let category = BUILTIN_METADATA[id].category;
                categorized.entry(category).or_default().push(name.clone());
            }
        }
        categorized
            .into_iter()
            .map(|(category, mut names)| {
                names.sort();
                (category.name(), names)
            })
            .collect()
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

macro_rules! builtin_metadata {
    ($category:ident, $policy:ident) => {
        BuiltinMetadata::new(BuiltinCategory::$category, BuiltinPolicy::$policy)
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
                $metadata:expr
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

            pub const fn metadata(self) -> BuiltinMetadata {
                match self {
                    $(
                        $(#[$m])*
                        BuiltinEnum::$VAR => $metadata,
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

        pub const BUILTIN_METADATA: &[BuiltinMetadata] = &[
            $(
                $(#[$m])*
                $metadata
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

            pub const METADATA: &'static [BuiltinMetadata] = BUILTIN_METADATA;

            pub(crate) const DEPTH_SUGAR: &'static [BuiltinDepthSugar] = BUILTIN_DEPTH_SUGAR;

            #[inline]
            pub fn name_from_id(id: u16) -> Option<&'static str> {
                Self::NAMES.get(usize::from(id)).copied()
            }

            #[inline]
            pub fn usage_from_id(id: u16) -> Option<&'static str> {
                Self::USAGES.get(usize::from(id)).copied()
            }

            #[inline]
            pub fn arity_from_id(id: u16) -> Option<BuiltinCallArity> {
                Self::SIGNATURES.get(usize::from(id)).map(|signature| signature.arity)
            }

            #[inline]
            pub fn metadata_from_id(id: u16) -> Option<BuiltinMetadata> {
                Self::METADATA.get(usize::from(id)).copied()
            }

            pub fn doc_for_name(&self, name: &str) -> Option<crate::doc::DocTopic> {
                self.name_to_id
                    .get(name)
                    .and_then(|id| u16::try_from(*id).ok())
                    .and_then(Self::doc_for_id)
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
    // Core =========================================================
    (BFN, Bfn, "bfn", "bfn[]", sig!(arity!(0)), with_context(core::bfn), builtin_metadata!(Core, REQUIRED_CONTEXTUAL)),
    (CHR, Chr, "chr", "chr[xs]", sig!(arity!(1)), plain(core::chr), builtin_metadata!(Core, PURE)),
    (ORD, Ord, "ord", "ord[xs]", sig!(arity!(1)), plain(core::ord), builtin_metadata!(Core, PURE)),
    (INT, Int, "int", "int[x], int[x;base]", sig!(arity!(1, 2)), plain(core::int), builtin_metadata!(Core, PURE)),
    (FLOAT, Float, "float", "float[x]", sig!(arity!(1)), plain(core::float), builtin_metadata!(Core, PURE)),
    (BIN, Bin, "bin", "bin[xs;prefix?]", sig!(arity!(1, 2)), plain(core::bin), builtin_metadata!(Core, PURE)),
    (OCT, Oct, "oct", "oct[xs;prefix?]", sig!(arity!(1, 2)), plain(core::oct), builtin_metadata!(Core, PURE)),
    (HEX, Hex, "hex", "hex[xs;prefix?]", sig!(arity!(1, 2)), plain(core::hex), builtin_metadata!(Core, PURE)),
    (HASH, Hash, "hash", "hash[x]", sig!(arity!(1)), plain(core::hash), builtin_metadata!(Core, PURE)),
    (ASSERT, Assert, "assert", "assert[condition;message?;`context]", sig!(arity!(1, 2), named ASSERT_NAMED_ARGS), plain(core::assert_condition), builtin_metadata!(Core, PURE)),
    (ASSERT_EQ, AssertEq, "assert_eq", "assert_eq[actual;expected;message?;`context]", sig!(arity!(2, 3), named ASSERT_NAMED_ARGS), plain(core::assert_equal), builtin_metadata!(Core, PURE)),
    (RAISE, Raise, "raise", "raise[]; raise[msg]", sig!(arity!(0, 1)), plain(core::raise), builtin_metadata!(Core, PURE)),
    (ARGV, Argv, "argv", "argv[]", sig!(arity!(0)), with_context(cli::argv), builtin_metadata!(Core, REQUIRED_CONTEXTUAL)),
    (ARGPARSE, Argparse, "argparse", "argparse[spec;args]", sig!(arity!(2)), with_context(cli::argparse), builtin_metadata!(Core, PURE_CONTEXTUAL)),
    (CLIARGS, Cliargs, "cliargs", "cliargs[spec]", sig!(arity!(1)), with_context(cli::cliargs), builtin_metadata!(Core, CONSTRAINED_EFFECT)),

    (ECHO, Echo, "echo", "echo[value*;`sep]", sig!(arity!(0..), named ECHO_NAMED_ARGS), plain(core::echo), builtin_metadata!(Core, CONSTRAINED_EFFECT)),
    (E, E, "E", "E[value*;`sep]", sig!(arity!(0..), named ECHO_NAMED_ARGS, alias Echo), plain(core::echo), builtin_metadata!(Core, CONSTRAINED_EFFECT)), // alias of echo
    (PRINT, Print, "print", "print[value*]", sig!(arity!(0..)), plain(core::print), builtin_metadata!(Core, CONSTRAINED_EFFECT)),
    (INPUT, Input, "input", "input[prompt?]", sig!(arity!(0, 1)), plain(core::input), builtin_metadata!(Core, CONSTRAINED_EFFECT)),
    #[cfg(not(target_arch = "wasm32"))]
    (EXEC, Exec, "exec", "exec[parts+;`stdin;`cwd;`env;`timeout;`check]", sig!(arity!(1..), named EXEC_NAMED_ARGS), plain(core::exec), builtin_metadata!(Exec, UNCONSTRAINED_EFFECT)),

    // ENCODING =========================================================
    (DECODE, Decode, "decode", "decode[bytes;codec;mode?]", sig!(arity!(2, 3)), plain(encoding::decode), builtin_metadata!(Encoding, PURE)),
    (ENCODE, Encode, "encode", "encode[text;codec;mode?]", sig!(arity!(2, 3)), plain(encoding::encode), builtin_metadata!(Encoding, PURE)),
    (VALIDBYTES, ValidBytes, "bytes?", "bytes?[x]", sig!(arity!(1)), plain(encoding::is_valid_bytes), builtin_metadata!(Encoding, PURE)),

    // FILE IO =========================================================
    #[cfg(not(target_arch = "wasm32"))]
    {
        (OPEN, Open, "open", "open[path;`r;`w;`a;`t;`c;`cn]", sig!(arity!(1), named OPEN_NAMED_ARGS), plain(io::open), builtin_metadata!(FileIO, UNCONSTRAINED_EFFECT)),
        (FEXISTS_Q, FexistsQ, "fexists?", "fexists?[path]", sig!(arity!(1)), plain(io::fexists), builtin_metadata!(FileIO, UNCONSTRAINED_EFFECT)),
        (MKDIR, Mkdir, "mkdir", "mkdir[path]", sig!(arity!(1)), plain(io::mkdir), builtin_metadata!(FileIO, UNCONSTRAINED_EFFECT)),
        (FSIZE, Fsize, "fsize", "fsize[path]", sig!(arity!(1)), plain(io::fsize), builtin_metadata!(FileIO, UNCONSTRAINED_EFFECT)),
        (FWRITE, Fwrite, "fwrite", "fwrite[stream;bytes]", sig!(arity!(2)), plain(io::fwrite), builtin_metadata!(FileIO, UNCONSTRAINED_EFFECT)),
        (FWRITET, Fwritet, "fwritet", "fwritet[stream;text]", sig!(arity!(2)), plain(io::fwritet), builtin_metadata!(FileIO, UNCONSTRAINED_EFFECT)),
        (FREAD, Fread, "fread", "fread[stream;len?]", sig!(arity!(1, 2)), plain(io::fread), builtin_metadata!(FileIO, UNCONSTRAINED_EFFECT)),
        (FREADT, Freadt, "freadt", "freadt[stream;len?]", sig!(arity!(1, 2)), plain(io::freadt), builtin_metadata!(FileIO, UNCONSTRAINED_EFFECT)),
        (FREADTLN, Freadtln, "freadtln", "freadtln[stream]", sig!(arity!(1)), plain(io::freadtln), builtin_metadata!(FileIO, UNCONSTRAINED_EFFECT)),
        (FREADTLNS, Freadtlns, "freadtlns", "freadtlns[stream]", sig!(arity!(1)), plain(io::freadtlns), builtin_metadata!(FileIO, UNCONSTRAINED_EFFECT)),
        (FSEEK, Fseek, "fseek", "fseek[stream;offset;whence?]", sig!(arity!(2, 3)), plain(io::fseek), builtin_metadata!(FileIO, UNCONSTRAINED_EFFECT)),
        (FTELL, Ftell, "ftell", "ftell[stream]", sig!(arity!(1)), plain(io::ftell), builtin_metadata!(FileIO, UNCONSTRAINED_EFFECT)),
        (FCLOSE, Fclose, "fclose", "fclose[stream]", sig!(arity!(1)), plain(io::fclose), builtin_metadata!(FileIO, UNCONSTRAINED_EFFECT)),
    },

    // Meta =========================================================
    (LEN, Len, "len", "len[xs]", sig!(arity!(1)), plain(meta::len), builtin_metadata!(Meta, REQUIRED)),
    (SHAPE, Shape, "shape", "shape[xs]", sig!(arity!(1)), plain(meta::shape), builtin_metadata!(Meta, PURE)),
    (DEPTH, Depth, "depth", "depth[xs]", sig!(arity!(1)), plain(meta::depth), builtin_metadata!(Meta, PURE)),
    (UNIFORM_Q, UniformQ, "uniform?", "uniform?[xs]", sig!(arity!(1)), plain(meta::is_uniform), builtin_metadata!(Meta, PURE)),

    // List =========================================================
    (SUM, Sum, "sum", "sum[xs*]", sig!(arity!(0..)), plain(list::sum), builtin_metadata!(List, PURE)),
    (PRODUCT, Product, "product", "product[xs*]", sig!(arity!(0..)), plain(list::product), builtin_metadata!(List, PURE)),
    (MIN, Min, "min", "min[xs], min[xs;ys+]", sig!(arity!(1..)), plain(list::min), builtin_metadata!(List, PURE)),
    (MAX, Max, "max", "max[xs], max[xs;ys+]", sig!(arity!(1..)), plain(list::max), builtin_metadata!(List, PURE)),
    (FLATTEN, Flatten, "flatten", "flatten[xs]", sig!(arity!(1)), plain(list::flatten), builtin_metadata!(List, PURE)),
    (REVERSE, Reverse, "reverse", "reverse[xs]", sig!(arity!(1)), plain(list::reverse), builtin_metadata!(List, PURE)),
    (V, V, "V", "V[xs]", sig!(arity!(1), alias Reverse), plain(list::reverse), builtin_metadata!(List, PURE)), // alias of reverse
    (SORT, Sort, "sort", "sort[xs]", sig!(arity!(1)), plain(list::sort), builtin_metadata!(List, PURE)),
    (SPLIT, Split, "split", "split[xs;opts?]", sig!(arity!(1, 2), named MAXSPLIT_NAMED_ARGS), plain(list::split), builtin_metadata!(List, PURE)),
    (FIND, Find, "find", "find[xs;elem;threshold?;d?]", sig!(arity!(2, 3, 4)), plain(list::find), builtin_metadata!(List, PURE), BuiltinDepthSugar::AppendDefaultInt { required_argc: 2, optional_argc: 3, default: 1 }),
    (RFIND, RFind, "rfind", "rfind[xs;elem;threshold?;d?]", sig!(arity!(2, 3, 4)), plain(list::rfind), builtin_metadata!(List, PURE), BuiltinDepthSugar::AppendDefaultInt { required_argc: 2, optional_argc: 3, default: 1 }),
    (ZIP, Zip, "zip", "zip[xs;ys;d?]", sig!(arity!(2, 3)), plain(list::zip), builtin_metadata!(List, PURE), BuiltinDepthSugar::Append { non_depth_argc: 2 }),

    // List Gen =========================================================
    (ALLOC, Alloc, "alloc", "alloc[shape], alloc[shape;x]", sig!(arity!(1, 2)), plain(listgen::alloc), builtin_metadata!(ListGen, PURE)),
    (TIL, Til, "til", "til[shape]", sig!(arity!(1)), plain(listgen::til), builtin_metadata!(ListGen, PURE)),
    (IOTA, Iota, "iota", "iota[shape]", sig!(arity!(1)), plain(listgen::iota), builtin_metadata!(ListGen, PURE)),
    (RANGE, Range, "range", "range[start;end], range[start;end;step]", sig!(arity!(2, 3)), plain(listgen::range), builtin_metadata!(ListGen, PURE)),

    (RESHAPE, Reshape, "reshape", "reshape[xs;shape]", sig!(arity!(2)), plain(listgen::reshape), builtin_metadata!(ListGen, PURE)),
    (R, R, "R", "R[xs;shape]", sig!(arity!(2), alias Reshape), plain(listgen::reshape), builtin_metadata!(ListGen, PURE)), // alias of reshape
    (TRANSPOSE, Transpose, "transpose", "transpose[x;axes?]", sig!(arity!(1, 2)), plain(listgen::transpose::transpose), builtin_metadata!(Mat, PURE)),
    (TP, TP, "TP", "TP[x;axes?]", sig!(arity!(1, 2), alias Transpose), plain(listgen::transpose::transpose), builtin_metadata!(Mat, PURE)), // alias of transpose

    (REPEAT, Repeat, "repeat", "repeat[xs;n]", sig!(arity!(2)), plain(listgen::repeat), builtin_metadata!(ListGen, PURE)),
    (WHERE, Where, "where", "where[xs]", sig!(arity!(1)), plain(listgen::wq_where), builtin_metadata!(ListGen, PURE)),
    (Z, Z, "Z", "Z[xs]", sig!(arity!(1), alias Where), plain(listgen::wq_where), builtin_metadata!(ListGen, PURE)), // alias of where

    // Higher-order =========================================================
    (APPLY, Apply, "apply", "apply[fs;x]", sig!(arity!(2)), with_context(ho::apply), builtin_metadata!(HigherOrder, PURE_CONTEXTUAL)),
    (MAP, Map, "map", "map[xs;f;d?]", sig!(arity!(2, 3)), with_context(ho::map), builtin_metadata!(HigherOrder, PURE_CONTEXTUAL), BuiltinDepthSugar::Append { non_depth_argc: 2 }),
    (M, M, "M", "M[xs;f;d?]", sig!(arity!(2, 3), alias Map), with_context(ho::map), builtin_metadata!(HigherOrder, PURE_CONTEXTUAL), BuiltinDepthSugar::Append { non_depth_argc: 2 }), // alias of map
    (FOLD, Fold, "fold", "fold[xs;f;i?]", sig!(arity!(2, 3)), with_context(ho::fold), builtin_metadata!(HigherOrder, PURE_CONTEXTUAL)),
    (REDUCE, Reduce, "reduce", "reduce[xs;f;i?]", sig!(arity!(2, 3), alias Fold), with_context(ho::fold), builtin_metadata!(HigherOrder, PURE_CONTEXTUAL)), // alias of fold
    (SCAN, Scan, "scan", "scan[xs;f;acc?]", sig!(arity!(2, 3)), with_context(ho::scan), builtin_metadata!(HigherOrder, PURE_CONTEXTUAL)),
    (RSCAN, RScan, "rscan", "rscan[xs;f;acc?]", sig!(arity!(2, 3)), with_context(ho::rscan), builtin_metadata!(HigherOrder, PURE_CONTEXTUAL)),
    (ANY, Any, "any", "any[xs;f;d?]", sig!(arity!(2, 3)), with_context(ho::any), builtin_metadata!(HigherOrder, PURE_CONTEXTUAL), BuiltinDepthSugar::Append { non_depth_argc: 2 }),
    (ALL, All, "all", "all[xs;f;d?]", sig!(arity!(2, 3)), with_context(ho::all), builtin_metadata!(HigherOrder, PURE_CONTEXTUAL), BuiltinDepthSugar::Append { non_depth_argc: 2 }),
    (FILTER, Filter, "filter", "filter[xs;f]", sig!(arity!(2)), with_context(ho::filter), builtin_metadata!(HigherOrder, PURE_CONTEXTUAL)),

    (ZIPW, ZipW, "zipw", "zipw[xs;ys;f;d?]", sig!(arity!(3, 4)), with_context(ho::zipw), builtin_metadata!(HigherOrder, PURE_CONTEXTUAL), BuiltinDepthSugar::Append { non_depth_argc: 3 }),
    (SPLITW, SplitW, "splitw", "splitw[xs;f;`m]", sig!(arity!(2), named MAXSPLIT_NAMED_ARGS), with_context(ho::splitw), builtin_metadata!(HigherOrder, PURE_CONTEXTUAL)),
    (FINDW, FindW, "findw", "findw[xs;f;threshold?;d?]", sig!(arity!(2, 3, 4)), with_context(ho::findw), builtin_metadata!(HigherOrder, PURE_CONTEXTUAL), BuiltinDepthSugar::AppendDefaultInt { required_argc: 2, optional_argc: 3, default: 1 }),
    (RFINDW, RFindW, "rfindw", "rfindw[xs;f;threshold?;d?]", sig!(arity!(2, 3, 4)), with_context(ho::rfindw), builtin_metadata!(HigherOrder, PURE_CONTEXTUAL), BuiltinDepthSugar::AppendDefaultInt { required_argc: 2, optional_argc: 3, default: 1 }),

    // Dict =========================================================
    (KEYS, Keys, "keys", "keys[dct]", sig!(arity!(1)), plain(dict::keys), builtin_metadata!(Dict, PURE)),
    (IDX_TO_KEY, IdxToKey, "itk", "itk[dct;i]", sig!(arity!(2)), plain(dict::idx_to_key), builtin_metadata!(Dict, PURE)),
    (KEY_TO_IDX, KeyToIdx, "kti", "kti[dct;k]", sig!(arity!(2)), plain(dict::key_to_idx), builtin_metadata!(Dict, PURE)),

    // Set ==========================================================
    (UNIQUE, Unique, "unique", "unique[xs]", sig!(arity!(1)), plain(set::unique), builtin_metadata!(Set, PURE)),
    (COUNTS, Counts, "counts", "counts[xs]", sig!(arity!(1)), plain(set::counts), builtin_metadata!(Set, PURE)),
    (UNION, Union, "union", "union[xs;ys]", sig!(arity!(2)), plain(set::union), builtin_metadata!(Set, PURE)),
    (INTERSECT, Intersect, "intersect", "intersect[xs;ys]", sig!(arity!(2)), plain(set::intersect), builtin_metadata!(Set, PURE)),
    (WITHOUT, Without, "without", "without[xs;ys]", sig!(arity!(2)), plain(set::without), builtin_metadata!(Set, PURE)),
    (SYMDIFF, Symdiff, "symdiff", "symdiff[xs;ys]", sig!(arity!(2)), plain(set::symdiff), builtin_metadata!(Set, PURE)),
    (SUB_Q, SubQ, "sub?", "sub?[xs;ys]", sig!(arity!(2)), plain(set::subset), builtin_metadata!(Set, PURE)),
    (SUPER_Q, SuperQ, "super?", "super?[xs;ys]", sig!(arity!(2)), plain(set::superset), builtin_metadata!(Set, PURE)),
    (P_SUB_Q, PSubQ, "psub?", "psub?[xs;ys]", sig!(arity!(2)), plain(set::proper_subset), builtin_metadata!(Set, PURE)),
    (P_SUPER_Q, PSuperQ, "psuper?", "psuper?[xs;ys]", sig!(arity!(2)), plain(set::proper_superset), builtin_metadata!(Set, PURE)),
    (MEMBER_Q, MemberQ, "member?", "member?[xs;ys]", sig!(arity!(2)), plain(set::member), builtin_metadata!(Set, PURE)),
    (CART, Cart, "cart", "cart[xs;ys]", sig!(arity!(2)), plain(set::carproduct), builtin_metadata!(Set, PURE)),
    (IN_Q, InQ, "in?", "in?[x;xs;d?]", sig!(arity!(2, 3)), plain(set::in_), builtin_metadata!(Set, PURE), BuiltinDepthSugar::Append { non_depth_argc: 2 }),
    (HAS_Q, HasQ, "has?", "has?[xs;x;d?]", sig!(arity!(2, 3)), plain(set::has), builtin_metadata!(Set, PURE), BuiltinDepthSugar::Append { non_depth_argc: 2 }),
    (DISJOINT_Q, DisjointQ, "disjoint?", "disjoint?[xs;ys]", sig!(arity!(2)), plain(set::disjoint), builtin_metadata!(Set, PURE)),
    (MULTIPLICITY, Multiplicity, "multiplicity", "multiplicity[x;xs]", sig!(arity!(2)), plain(set::multiplicity), builtin_metadata!(Set, PURE)),

    // Logical ======================================================
    (NOT, Not, "not", "not[xs]", sig!(arity!(1)), plain(logical::not), builtin_metadata!(Logical, PURE)),
    (BXOR, Bxor, "bxor", "bxor[xs;ys+]", sig!(arity!(2..)), plain(logical::bxor), builtin_metadata!(Logical, PURE)),

    (BAND, Band, "band", "band[xs;ys+]", sig!(arity!(2..)), plain(logical::band), builtin_metadata!(Logical, PURE)),
    (BOR, Bor, "bor", "bor[xs;ys+]", sig!(arity!(2..)), plain(logical::bor), builtin_metadata!(Logical, PURE)),

    (SHL, Shl, "shl", "shl[xs;shift+]", sig!(arity!(2..)), plain(logical::shl), builtin_metadata!(Logical, PURE)),
    (SHR, Shr, "shr", "shr[xs;shift+]", sig!(arity!(2..)), plain(logical::shr), builtin_metadata!(Logical, PURE)),

    // Math =========================================================
    (NEG, Neg, "neg", "neg[xs]", sig!(arity!(1)), plain(math::neg), builtin_metadata!(Math, PURE)),
    (ABS, Abs, "abs", "abs[xs]", sig!(arity!(1)), plain(math::abs), builtin_metadata!(Math, PURE)),
    (SGN, Sgn, "sgn", "sgn[xs]", sig!(arity!(1)), plain(math::sgn), builtin_metadata!(Math, PURE)),
    (SQRT, Sqrt, "sqrt", "sqrt[xs]", sig!(arity!(1)), plain(math::sqrt), builtin_metadata!(Math, PURE)),
    (EXP, Exp, "exp", "exp[xs]", sig!(arity!(1)), plain(math::exp), builtin_metadata!(Math, PURE)),
    (LN, Ln, "ln", "ln[xs]", sig!(arity!(1)), plain(math::ln), builtin_metadata!(Math, PURE)),
    (LOG2, Log2, "log2", "log2[xs]", sig!(arity!(1)), plain(math::log2), builtin_metadata!(Math, PURE)),
    (LOG10, Log10, "log10", "log10[xs]", sig!(arity!(1)), plain(math::log10), builtin_metadata!(Math, PURE)),
    (FLOOR, Floor, "floor", "floor[xs;d?]", sig!(arity!(1, 2)), plain(math::floor), builtin_metadata!(Math, PURE)),
    (CEIL, Ceil, "ceil", "ceil[xs;d?]", sig!(arity!(1, 2)), plain(math::ceil), builtin_metadata!(Math, PURE)),
    (ROUND, Round, "round", "round[xs;d?]", sig!(arity!(1, 2)), plain(math::round), builtin_metadata!(Math, PURE)),

    (SIN, Sin, "sin", "sin[xs]", sig!(arity!(1)), plain(math::sin), builtin_metadata!(Math, PURE)),
    (COS, Cos, "cos", "cos[xs]", sig!(arity!(1)), plain(math::cos), builtin_metadata!(Math, PURE)),
    (TAN, Tan, "tan", "tan[xs]", sig!(arity!(1)), plain(math::tan), builtin_metadata!(Math, PURE)),
    (SEC, Sec, "sec", "sec[xs]", sig!(arity!(1)), plain(math::sec), builtin_metadata!(Math, PURE)),
    (CSC, Csc, "csc", "csc[xs]", sig!(arity!(1)), plain(math::csc), builtin_metadata!(Math, PURE)),
    (COT, Cot, "cot", "cot[xs]", sig!(arity!(1)), plain(math::cot), builtin_metadata!(Math, PURE)),
    (ARCSIN, Arcsin, "arcsin", "arcsin[xs]", sig!(arity!(1)), plain(math::arcsin), builtin_metadata!(Math, PURE)),
    (ARCCOS, Arccos, "arccos", "arccos[xs]", sig!(arity!(1)), plain(math::arccos), builtin_metadata!(Math, PURE)),
    (ARCTAN, Arctan, "arctan", "arctan[xs]", sig!(arity!(1)), plain(math::arctan), builtin_metadata!(Math, PURE)),
    (SINH, Sinh, "sinh", "sinh[xs]", sig!(arity!(1)), plain(math::sinh), builtin_metadata!(Math, PURE)),
    (COSH, Cosh, "cosh", "cosh[xs]", sig!(arity!(1)), plain(math::cosh), builtin_metadata!(Math, PURE)),
    (TANH, Tanh, "tanh", "tanh[xs]", sig!(arity!(1)), plain(math::tanh), builtin_metadata!(Math, PURE)),
    (ARCSINH, Arcsinh, "arcsinh", "arcsinh[xs]", sig!(arity!(1)), plain(math::arcsinh), builtin_metadata!(Math, PURE)),
    (ARCCOSH, Arccosh, "arccosh", "arccosh[xs]", sig!(arity!(1)), plain(math::arccosh), builtin_metadata!(Math, PURE)),
    (ARCTANH, Arctanh, "arctanh", "arctanh[xs]", sig!(arity!(1)), plain(math::arctanh), builtin_metadata!(Math, PURE)),
    (LOG, Log, "log", "log[x;a]", sig!(arity!(2)), plain(math::log), builtin_metadata!(Math, PURE)),
    (ARCTAN2, Arctan2, "arctan2", "arctan2[y;x]", sig!(arity!(2)), plain(math::arctan2), builtin_metadata!(Math, PURE)),

    (ERF, Erf, "erf", "erf[xs]", sig!(arity!(1)), plain(math::erf), builtin_metadata!(Math, PURE)),
    (ERFC, Erfc, "erfc", "erfc[xs]", sig!(arity!(1)), plain(math::erfc), builtin_metadata!(Math, PURE)),
    (GAMMA, Gamma, "gamma", "gamma[xs]", sig!(arity!(1)), plain(math::gamma), builtin_metadata!(Math, PURE)),
    (LNGAMMA, Lngamma, "lngamma", "lngamma[xs]", sig!(arity!(1)), plain(math::lngamma), builtin_metadata!(Math, PURE)),
    (SI, Si, "si", "si[xs]", sig!(arity!(1)), plain(math::si), builtin_metadata!(Math, PURE)),
    (CI, Ci, "ci", "ci[xs]", sig!(arity!(1)), plain(math::ci), builtin_metadata!(Math, PURE)),
    (EI, Ei, "ei", "ei[xs]", sig!(arity!(1)), plain(math::ei), builtin_metadata!(Math, PURE)),
    (EN, En, "en", "en[n;xs]", sig!(arity!(2)), plain(math::en), builtin_metadata!(Math, PURE)),
    (ELLPK, Ellpk, "ellpk", "ellpk[xs]", sig!(arity!(1)), plain(math::ellpk), builtin_metadata!(Math, PURE)),
    (ELLPE, Ellpe, "ellpe", "ellpe[xs]", sig!(arity!(1)), plain(math::ellpe), builtin_metadata!(Math, PURE)),
    (ELLIK, Ellik, "ellik", "ellik[phi;m]", sig!(arity!(2)), plain(math::ellik), builtin_metadata!(Math, PURE)),
    (ELLIE, Ellie, "ellie", "ellie[phi;m]", sig!(arity!(2)), plain(math::ellie), builtin_metadata!(Math, PURE)),
    (HEAVISIDE, Heaviside, "heaviside", "heaviside[xs]", sig!(arity!(1)), plain(math::heaviside), builtin_metadata!(Math, PURE)),
    (DELTA, Delta, "delta", "delta[xs]", sig!(arity!(1)), plain(math::delta), builtin_metadata!(Math, PURE)),

    // Rand
    (RAND, Rand, "rand", "rand[]; rand[upper]; rand[lower;upper]", sig!(arity!(0, 1, 2)), with_context(random::rand), builtin_metadata!(Rand, CONSTRAINED_EFFECT)),
    (RNG, Rng, "rng", "rng[seed]", sig!(arity!(1)), plain(random::rng), builtin_metadata!(Rand, CONSTRAINED_EFFECT)),

    // Complex
    (COMPLEX, Complex, "complex", "complex[re;im]", sig!(arity!(2)), plain(complex::complex), builtin_metadata!(Complex, PURE)),
    (RE, Re, "re", "re[x]", sig!(arity!(1)), plain(complex::real), builtin_metadata!(Complex, PURE)),
    (IM, Im, "im", "im[x]", sig!(arity!(1)), plain(complex::imag), builtin_metadata!(Complex, PURE)),
    (CONJ, Conj, "conj", "conj[x]", sig!(arity!(1)), plain(complex::conj), builtin_metadata!(Complex, PURE)),

    // Fraction
    (FRACTION, Fraction, "fraction", "fraction[xs;lim?]", sig!(arity!(1, 2)), plain(fraction::fraction), builtin_metadata!(Fraction, PURE)),
    (FRACTIONL, Fractionl, "fractionl", "fractionl[xs]", sig!(arity!(1)), plain(fraction::fractionl), builtin_metadata!(Fraction, PURE)),

    // CAS
    (EQ, Eq, "eq", "eq[lhs;rhs]", sig!(arity!(2)), plain(cas::eq), builtin_metadata!(Cas, PURE)),
    (ZERO, Zero, "zero", "zero[expr]", sig!(arity!(1)), plain(cas::zero), builtin_metadata!(Cas, PURE)),
    (NONZERO, Nonzero, "nonzero", "nonzero[expr]", sig!(arity!(1)), plain(cas::nonzero), builtin_metadata!(Cas, PURE)),
    (POSITIVE, Positive, "positive", "positive[expr]", sig!(arity!(1)), plain(cas::positive), builtin_metadata!(Cas, PURE)),
    (NEGATIVE, Negative, "negative", "negative[expr]", sig!(arity!(1)), plain(cas::negative), builtin_metadata!(Cas, PURE)),
    (NONNEGATIVE, Nonnegative, "nonnegative", "nonnegative[expr]", sig!(arity!(1)), plain(cas::nonnegative), builtin_metadata!(Cas, PURE)),
    (REAL, Real, "real", "real[expr]", sig!(arity!(1)), plain(cas::real), builtin_metadata!(Cas, PURE)),
    (INTEGER, Integer, "integer", "integer[expr]", sig!(arity!(1)), plain(cas::integer), builtin_metadata!(Cas, PURE)),
    (SIMPLIFY, Simplify, "simplify", "simplify[expr]", sig!(arity!(1)), plain(cas::simplify), builtin_metadata!(Cas, PURE)),
    (REWRITE, Rewrite, "rewrite", "rewrite[expr]", sig!(arity!(1)), plain(cas::rewrite), builtin_metadata!(Cas, PURE)),
    (NUMERIC, Numeric, "numeric", "numeric[expr], numeric[expr;`name:val...]", sig!(arity!(1), defer), plain(cas::numeric), builtin_metadata!(Cas, PURE)),
    (DIFF, Diff, "diff", "diff[expr;var?]", sig!(arity!(1, 2)), plain(cas::diff), builtin_metadata!(Cas, PURE)),
    (D, D, "D", "D[expr;var?]", sig!(arity!(1, 2), alias Diff), plain(cas::diff), builtin_metadata!(Cas, PURE)), // alias of diff
    (SUBSTITUTE, Substitute, "substitute", "substitute[expr;eqs], substitute[expr;var;val], substitute[expr;`name:val...]", sig!(arity!(1, 2, 3), defer), plain(cas::substitute), builtin_metadata!(Cas, PURE)),
    (EXPAND, Expand, "expand", "expand[expr]", sig!(arity!(1)), plain(cas::expand), builtin_metadata!(Cas, PURE)),
    (FACTOR_COMMON, FactorCommon, "factor_common", "factor_common[expr]", sig!(arity!(1)), plain(cas::factor_common), builtin_metadata!(Cas, PURE)),
    (FACTOR, Factor, "factor", "factor[expr], factor[expr;var], factor[expr;1], factor[expr;1;var]", sig!(arity!(1, 2, 3)), plain(cas::factor_poly), builtin_metadata!(Cas, PURE)),
    (INTEGRATE, Integrate, "integrate", "integrate[expr], integrate[expr;var], integrate[expr;var;lower;upper]", sig!(arity!(1, 2, 4)), plain(cas::integrate), builtin_metadata!(Cas, PURE)),
    (I, I, "I", "I[expr], I[expr;var], I[expr;var;lower;upper]", sig!(arity!(1, 2, 4), alias Integrate), plain(cas::integrate), builtin_metadata!(Cas, PURE)), // alias of integrate
    (LIMIT, Limit, "limit", "limit[expr;point;`d], limit[expr;var;point;`d]", sig!(arity!(2..), named LIMIT_NAMED_ARGS), plain(cas::limit), builtin_metadata!(Cas, PURE)),
    (SOLVE, Solve, "solve", "solve[expr;`assuming;`domain], solve[expr;var;`assuming;`domain], solve[eq;var;`assuming;`domain]", sig!(arity!(1, 2), named SOLVE_NAMED_ARGS), plain(cas::solve), builtin_metadata!(Cas, PURE)),
    (SOLVE_SYSTEM, SolveSystem, "solve_system", "solve_system[eqs;`assuming], solve_system[eqs;vars;`assuming]", sig!(arity!(1, 2), named SOLVE_SYSTEM_NAMED_ARGS), plain(cas::solve_system), builtin_metadata!(Cas, PURE)),
    (BRENT, Brent, "brent", "brent[expr;a;b], brent[expr;a;b;tol], brent[expr;a;b;tol;max_iter], brent[eq;a;b]", sig!(arity!(3, 4, 5)), plain(cas::brent), builtin_metadata!(Cas, PURE)),
    (NEWTON, Newton, "newton", "newton[expr;x0], newton[expr;x0;tol], newton[expr;x0;tol;max_iter], newton[eq;x0]", sig!(arity!(2, 3, 4)), plain(cas::newton), builtin_metadata!(Cas, PURE)),

    // String =========================================================
    (STR, Str, "str", "str[x]", sig!(arity!(1)), plain(string::to_str), builtin_metadata!(Str, PURE)),
    (GRAPHEMES, Graphemes, "graphemes", "graphemes[s]", sig!(arity!(1)), plain(string::graphemes), builtin_metadata!(Str, PURE)),
    (WS_Q, WsQ, "ws?", "ws?[c]", sig!(arity!(1)), plain(string::is_whitespace), builtin_metadata!(Str, PURE)),
    (WORDS, Words, "words", "words[s]", sig!(arity!(1)), plain(string::words), builtin_metadata!(Str, PURE)),
    (TRIM, Trim, "trim", "trim[s]", sig!(arity!(1)), plain(string::trim), builtin_metadata!(Str, PURE)),
    (L_TRIM, LTrim, "ltrim", "ltrim[s]", sig!(arity!(1)), plain(string::trim_left), builtin_metadata!(Str, PURE)),
    (R_TRIM, RTrim, "rtrim", "rtrim[s]", sig!(arity!(1)), plain(string::trim_right), builtin_metadata!(Str, PURE)),

    // Type =========================================================
    (TYPE, Type, "type", "type[x]", sig!(arity!(1)), plain(wqtype::type_of), builtin_metadata!(Type, PURE)),
    (TAG, Tag, "tag", "tag[x]", sig!(arity!(1)), plain(wqtype::to_tag), builtin_metadata!(Type, PURE)),
    (BOOL, Bool, "bool", "bool[x]", sig!(arity!(1)), plain(wqtype::to_bool), builtin_metadata!(Type, PURE)),
    (CHAR, Char, "char", "char[x]", sig!(arity!(1)), plain(wqtype::to_char), builtin_metadata!(Type, PURE)),
    (ATOM_Q, AtomQ, "atom?", "atom?[x]", sig!(arity!(1)), plain(wqtype::is_atom), builtin_metadata!(Type, PURE)),
    (UNIT_Q, UnitQ, "unit?", "unit?[x]", sig!(arity!(1)), plain(wqtype::is_unit), builtin_metadata!(Type, PURE)),
    (U, U, "U", "U[x]", sig!(arity!(1), alias UnitQ), plain(wqtype::is_unit), builtin_metadata!(Type, PURE)), // alias of unit?
    (LIST, List, "list", "list[x]", sig!(arity!(1)), plain(wqtype::to_list), builtin_metadata!(Type, PURE)),
    (DICT, Dict, "dict", "dict[x]", sig!(arity!(1)), plain(wqtype::to_dict), builtin_metadata!(Type, PURE)),

    // Visualization =========================================================
    (SHOWTABLE, Showtable, "showtable", "showtable[table;`cols;`limit;`width;`style;`missing]", sig!(arity!(1), named SHOWTABLE_NAMED_ARGS), plain(viz::show_table), builtin_metadata!(Viz, CONSTRAINED_EFFECT)),
    (ASCIIPLOT, Asciiplot, "asciiplot",
        concat!("asciiplot[data+;`size;`width;`height;`xlim;`ylim;",
            "`x;`y;`symbols;`labels;`mode;`axes;`color;`grid;",
            "`samples;`theme;`complex;`unicode;",
            "`ticklabels;`title;`xlabel;`ylabel;`caption]"), sig!(arity!(1..), named ASCIIPLOT_NAMED_ARGS), with_context(viz::asciiplot), builtin_metadata!(Viz, CONSTRAINED_EFFECT)),

    // Language-required builtins ===================================
    (FMT, Fmt, "fmt", "fmt[template;v*]", sig!(arity!(1..)), plain(string::fmt), builtin_metadata!(Str, REQUIRED)),

    (OP_ADD, OpAdd, "+", "+[xs;ys+]", sig!(arity!(2..)), plain(op::op_add), builtin_metadata!(Math, REQUIRED)),
    (OP_SUB, OpSub, "-", "-[x], -[xs;ys+]", sig!(arity!(1..)), plain(op::op_sub), builtin_metadata!(Math, REQUIRED)),
    (OP_MUL, OpMul, "*", "*[xs;ys+]", sig!(arity!(2..)), plain(op::op_mul), builtin_metadata!(Math, REQUIRED)),
    (OP_DIV, OpDiv, "/", "/[xs;ys+]", sig!(arity!(2..)), plain(op::op_div), builtin_metadata!(Math, REQUIRED)),
    (OP_DIV_DOT, OpDivDot, "/.", "/.[xs;ys+]", sig!(arity!(2..)), plain(op::op_divdot), builtin_metadata!(Math, REQUIRED)),
    (OP_MOD, OpMod, "%", "%[xs;ys+]", sig!(arity!(2..)), plain(op::op_mod), builtin_metadata!(Math, REQUIRED)),
    (OP_FLOORDIV, OpFloorDiv, "/%", "/%[xs;ys+]", sig!(arity!(2..)), plain(op::op_floordiv), builtin_metadata!(Math, REQUIRED)),
    (OP_POWER, OpPower, "^", "^[xs;ys+]", sig!(arity!(2..)), plain(op::op_power), builtin_metadata!(Math, REQUIRED)),
    (OP_POWER_DOT, OpPowerDot, "^.", "^.[xs;ys+]", sig!(arity!(2..)), plain(op::op_power_dot), builtin_metadata!(Math, REQUIRED)),
    (OP_MATMUL, OpMatmul, "**", "**[xs;ys+]", sig!(arity!(2..)), plain(op::op_matmul), builtin_metadata!(Mat, REQUIRED)),

    (OP_EQUAL, OpEqual, "=", "=[xs;ys+]", sig!(arity!(2..)), plain(op::op_equal), builtin_metadata!(Logical, REQUIRED)),
    (OP_EQUAL_DOT, OpEqualDot, "=.", "=.[xs;ys+]", sig!(arity!(2..)), plain(op::op_equal_dot), builtin_metadata!(Logical, REQUIRED)),
    (OP_TILDE, OpTilde, "~", "~[x], ~[xs;ys+]", sig!(arity!(1..)), plain(op::op_tilde), builtin_metadata!(Logical, REQUIRED)),
    (OP_TILDE_DOT, OpTildeDot, "~.", "~.[xs;ys+]", sig!(arity!(2..)), plain(op::op_tilde_dot), builtin_metadata!(Logical, REQUIRED)),
    (OP_LT, OpLt, "<", "<[xs;ys+]", sig!(arity!(2..)), plain(op::op_lt), builtin_metadata!(Logical, REQUIRED)),
    (OP_LTE, OpLte, "<=", "<=[xs;ys+]", sig!(arity!(2..)), plain(op::op_lte), builtin_metadata!(Logical, REQUIRED)),
    (OP_GT, OpGt, ">", ">[xs;ys+]", sig!(arity!(2..)), plain(op::op_gt), builtin_metadata!(Logical, REQUIRED)),
    (OP_GTE, OpGte, ">=", ">=[xs;ys+]", sig!(arity!(2..)), plain(op::op_gte), builtin_metadata!(Logical, REQUIRED)),

    (OP_CAT, OpCat, ",", ",[xs;ys+]", sig!(arity!(2..)), plain(op::op_cat), builtin_metadata!(List, REQUIRED)),
    (OP_SHARP, OpSharp, "#", "#[x]", sig!(arity!(1)), plain(op::op_sharp), builtin_metadata!(Meta, REQUIRED)),

}

const ECHO_NAMED_ARGS: &[&str] = &["sep"];
const ASSERT_NAMED_ARGS: &[&str] = &["context"];
const MAXSPLIT_NAMED_ARGS: &[&str] = &["m"];
const LIMIT_NAMED_ARGS: &[&str] = &["d"];
const SOLVE_NAMED_ARGS: &[&str] = &["assuming", "domain"];
const SOLVE_SYSTEM_NAMED_ARGS: &[&str] = &["assuming"];
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
    "unicode",
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
        for (index, (name, _)) in args.named_items().iter().enumerate() {
            if args.named_items()[..index]
                .iter()
                .any(|(previous, _)| previous == name)
            {
                let err = WqError::new(WqErrorType::Arity)
                    .msg(format!("duplicate named argument '{name}'"));
                return Err(if let Some(builtin) = BuiltinEnum::from_id(id) {
                    err.src(builtin)
                } else {
                    err
                });
            }
        }

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
        assert_eq!(Builtins::ENUMS.len(), Builtins::METADATA.len());
        assert_eq!(builtins.functions.len(), Builtins::SIGNATURES.len());
    }

    #[test]
    fn logical_builtin_names_exclude_parser_aliases() {
        assert!(Builtins::NAMES.contains(&"bxor"));
        assert!(!Builtins::NAMES.contains(&"xor"));
        assert!(!Builtins::NAMES.contains(&"and"));
        assert!(!Builtins::NAMES.contains(&"or"));
    }

    #[test]
    fn builtin_metadata_respects_registry_invariants() {
        let builtins = Builtins::new();
        let mut names = std::collections::HashSet::new();

        for ((builtin, name), metadata) in Builtins::ENUMS
            .iter()
            .zip(Builtins::NAMES)
            .zip(Builtins::METADATA)
        {
            assert!(names.insert(*name), "duplicate builtin name '{name}'");
            assert_eq!(builtin.metadata(), *metadata, "{builtin}");
            assert!(
                !metadata.policy.is_const_foldable()
                    || builtins.functions[usize::from(builtin.id())]
                        .as_plain()
                        .is_some(),
                "constant-foldable builtin '{name}' must be plain"
            );

            let canonical = builtin.canonical();
            if canonical != *builtin {
                assert_eq!(
                    metadata,
                    &canonical.metadata(),
                    "alias '{name}' must inherit canonical metadata"
                );
            }
        }

        for category in BuiltinCategory::ALL {
            assert!(
                Builtins::METADATA
                    .iter()
                    .any(|metadata| metadata.category == *category),
                "builtin category '{}' must not be empty",
                category.name()
            );
        }
    }

    #[test]
    fn builtin_presets_form_expected_lattice() {
        let all = Builtins::with_preset(BuiltinPreset::All);
        let pure = Builtins::with_preset(BuiltinPreset::Pure);
        let minimal = Builtins::with_preset(BuiltinPreset::Minimal);
        let constrained = Builtins::with_preset(BuiltinPreset::Constrained);

        for name in Builtins::NAMES {
            assert!(
                !minimal.is_enabled_name(name) || pure.is_enabled_name(name),
                "minimal builtin '{name}' must also be pure"
            );
            assert!(
                !pure.is_enabled_name(name) || constrained.is_enabled_name(name),
                "pure builtin '{name}' must also be constrained"
            );
            assert!(
                !constrained.is_enabled_name(name) || all.is_enabled_name(name),
                "constrained builtin '{name}' must also be available in all"
            );
        }
    }

    #[test]
    fn minimal_includes_builtin_introspection() {
        let minimal = Builtins::with_preset(BuiltinPreset::Minimal);
        let mut names = minimal.list_functions();
        names.sort();

        assert!(minimal.is_enabled_name("bfn"));
        assert_eq!(
            names,
            [
                "#", "%", "*", "**", "+", ",", "-", "/", "/%", "/.", "<", "<=", "=", "=.", ">",
                ">=", "^", "^.", "argv", "bfn", "fmt", "len", "~", "~.",
            ]
        );
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
            (BuiltinEnum::Assert, "1 2"),
            (BuiltinEnum::AssertEq, "2 3"),
            (BuiltinEnum::Raise, "0 1"),
            (BuiltinEnum::Argv, "0"),
            (BuiltinEnum::Argparse, "2"),
            (BuiltinEnum::Cliargs, "1"),
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
            (BuiltinEnum::Bxor, "2.."),
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
            (BuiltinEnum::Rng, "1"),
            (BuiltinEnum::Complex, "2"),
            (BuiltinEnum::Re, "1"),
            (BuiltinEnum::Im, "1"),
            (BuiltinEnum::Conj, "1"),
            (BuiltinEnum::Fraction, "1 2"),
            (BuiltinEnum::Fractionl, "1"),
            (BuiltinEnum::Eq, "2"),
            (BuiltinEnum::Zero, "1"),
            (BuiltinEnum::Nonzero, "1"),
            (BuiltinEnum::Positive, "1"),
            (BuiltinEnum::Negative, "1"),
            (BuiltinEnum::Nonnegative, "1"),
            (BuiltinEnum::Real, "1"),
            (BuiltinEnum::Integer, "1"),
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
                Builtins::BXOR,
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

        let duplicate_args = BuiltinFnArgs::with_named(
            SmallVec::new(),
            vec![
                (Arc::<str>::from("sep"), into_wq_string(",")),
                (Arc::<str>::from("sep"), into_wq_string(";")),
            ],
        );
        let err = builtins
            .validate_runtime_call_args(Builtins::E, &duplicate_args)
            .expect_err("duplicate named args should fail runtime validation");
        assert_eq!(err.err_type, WqErrorType::Arity);
        assert_eq!(err.msg.as_deref(), Some("duplicate named argument 'sep'"));

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
            vec![
                (Arc::<str>::from("width"), Value::Int(40)),
                (Arc::<str>::from("unicode"), Value::Bool(true)),
            ],
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
