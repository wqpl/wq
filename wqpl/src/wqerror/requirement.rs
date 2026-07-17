//! Semantic value requirements rendered with the public wq vocabulary.
//!
//! Keep articles and the word `expected` outside this model. Use
//! [`Requirement::string_literal`] for string values so diagnostics do not
//! hand-roll quotation marks. [`Requirement::literal`] is for canonical bare
//! values such as `inf`, `-inf`, and numerals.

use std::borrow::Cow;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Bound {
    Unbounded,
    Included(i128),
    Excluded(i128),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TypeName {
    Int,
    Float,
    Complex,
    Fraction,
    Bool,
    Char,
    Tag,
    String,
    List,
    Dict,
    #[cfg(not(target_arch = "wasm32"))]
    Stream,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClassName {
    Number,
    RealNumber,
    Atom,
    Callable,
    Pair,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Modifier {
    Positive,
    NonNegative,
    Finite,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Requirement {
    Type(TypeName),
    Class(ClassName),
    OneOf(Vec<Self>),
    List(Box<Self>),
    Dict {
        key: Box<Self>,
        value: Box<Self>,
    },
    IntRange {
        lower: Bound,
        upper: Bound,
    },
    Modified {
        modifier: Modifier,
        requirement: Box<Self>,
    },
    Literal(Cow<'static, str>),
    Phrase {
        singular: Cow<'static, str>,
        plural: Cow<'static, str>,
    },
}

impl Requirement {
    pub(crate) const INT: Self = Self::Type(TypeName::Int);
    pub(crate) const FLOAT: Self = Self::Type(TypeName::Float);
    pub(crate) const COMPLEX: Self = Self::Type(TypeName::Complex);
    pub(crate) const FRACTION: Self = Self::Type(TypeName::Fraction);
    pub(crate) const BOOL: Self = Self::Type(TypeName::Bool);
    pub(crate) const CHAR: Self = Self::Type(TypeName::Char);
    pub(crate) const TAG: Self = Self::Type(TypeName::Tag);
    pub(crate) const STRING: Self = Self::Type(TypeName::String);
    pub(crate) const LIST: Self = Self::Type(TypeName::List);
    pub(crate) const DICT: Self = Self::Type(TypeName::Dict);
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) const STREAM: Self = Self::Type(TypeName::Stream);

    pub(crate) const NUMBER: Self = Self::Class(ClassName::Number);
    pub(crate) const REAL_NUMBER: Self = Self::Class(ClassName::RealNumber);
    pub(crate) const ATOM: Self = Self::Class(ClassName::Atom);
    pub(crate) const CALLABLE: Self = Self::Class(ClassName::Callable);
    pub(crate) const PAIR: Self = Self::Class(ClassName::Pair);

    pub(crate) fn one_of(requirements: impl IntoIterator<Item = Self>) -> Self {
        let mut flattened = Vec::new();
        for requirement in requirements {
            match requirement {
                Self::OneOf(items) => {
                    for item in items {
                        if !flattened.contains(&item) {
                            flattened.push(item);
                        }
                    }
                }
                item if !flattened.contains(&item) => flattened.push(item),
                _ => {}
            }
        }
        debug_assert!(!flattened.is_empty(), "a union requirement is not empty");
        Self::OneOf(flattened)
    }

    pub(crate) fn list(item: Self) -> Self {
        Self::List(Box::new(item))
    }

    pub(crate) fn dict(key: Self, value: Self) -> Self {
        Self::Dict {
            key: Box::new(key),
            value: Box::new(value),
        }
    }

    pub(crate) const fn int_range(lower: Bound, upper: Bound) -> Self {
        Self::IntRange { lower, upper }
    }

    pub(crate) fn positive(requirement: Self) -> Self {
        Self::modified(Modifier::Positive, requirement)
    }

    pub(crate) fn non_negative(requirement: Self) -> Self {
        Self::modified(Modifier::NonNegative, requirement)
    }

    pub(crate) fn finite(requirement: Self) -> Self {
        Self::modified(Modifier::Finite, requirement)
    }

    fn modified(modifier: Modifier, requirement: Self) -> Self {
        Self::Modified {
            modifier,
            requirement: Box::new(requirement),
        }
    }

    pub(crate) fn literal(literal: impl Into<Cow<'static, str>>) -> Self {
        Self::Literal(literal.into())
    }

    pub(crate) fn string_literal(value: impl AsRef<str>) -> Self {
        Self::Literal(format!("{:?}", value.as_ref()).into())
    }

    pub(crate) fn phrase(
        singular: impl Into<Cow<'static, str>>,
        plural: impl Into<Cow<'static, str>>,
    ) -> Self {
        Self::Phrase {
            singular: singular.into(),
            plural: plural.into(),
        }
    }

    fn needs_element_scope(&self) -> bool {
        match self {
            Self::OneOf(_) | Self::Literal(_) => true,
            Self::Modified { requirement, .. } => requirement.needs_element_scope(),
            _ => false,
        }
    }

    fn can_modify_dict_member(&self) -> bool {
        match self {
            Self::Type(_) | Self::Class(_) => true,
            Self::Modified { requirement, .. } => requirement.can_modify_dict_member(),
            _ => false,
        }
    }

    fn render_dict_member(&self, member: &str) -> String {
        if self.can_modify_dict_member() {
            format!("{} {member}", self.render(GrammaticalNumber::Singular))
        } else {
            format!(
                "{member} that are {}",
                self.render(GrammaticalNumber::Plural)
            )
        }
    }

    fn render(&self, number: GrammaticalNumber) -> String {
        match self {
            Self::Type(name) => name.render(number).to_string(),
            Self::Class(name) => name.render(number).to_string(),
            Self::OneOf(requirements) => render_alternatives(
                requirements
                    .iter()
                    .map(|requirement| requirement.render(number)),
            ),
            Self::List(item) => {
                let list = match number {
                    GrammaticalNumber::Singular => "list",
                    GrammaticalNumber::Plural => "lists",
                };
                let item_text = item.render(GrammaticalNumber::Plural);
                if item.needs_element_scope() {
                    format!("{list} whose elements are {item_text}")
                } else {
                    format!("{list} of {item_text}")
                }
            }
            Self::Dict { key, value } => {
                let dict = match number {
                    GrammaticalNumber::Singular => "dict",
                    GrammaticalNumber::Plural => "dicts",
                };
                format!(
                    "{dict} with {} and {}",
                    key.render_dict_member("keys"),
                    value.render_dict_member("values"),
                )
            }
            Self::IntRange { lower, upper } => render_int_range(*lower, *upper, number),
            Self::Modified {
                modifier,
                requirement,
            } => match requirement.as_ref() {
                Self::OneOf(items) => render_alternatives(
                    items
                        .iter()
                        .map(|item| format!("{} {}", modifier.as_str(), item.render(number))),
                ),
                requirement => {
                    format!("{} {}", modifier.as_str(), requirement.render(number))
                }
            },
            Self::Literal(literal) => literal.to_string(),
            Self::Phrase { singular, plural } => match number {
                GrammaticalNumber::Singular => singular.to_string(),
                GrammaticalNumber::Plural => plural.to_string(),
            },
        }
    }
}

impl fmt::Display for Requirement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.render(GrammaticalNumber::Singular))
    }
}

#[derive(Debug, Clone, Copy)]
enum GrammaticalNumber {
    Singular,
    Plural,
}

impl TypeName {
    const fn render(self, number: GrammaticalNumber) -> &'static str {
        match (self, number) {
            (Self::Int, GrammaticalNumber::Singular) => "int",
            (Self::Int, GrammaticalNumber::Plural) => "ints",
            (Self::Float, GrammaticalNumber::Singular) => "float",
            (Self::Float, GrammaticalNumber::Plural) => "floats",
            (Self::Complex, GrammaticalNumber::Singular) => "complex",
            (Self::Complex, GrammaticalNumber::Plural) => "complex values",
            (Self::Fraction, GrammaticalNumber::Singular) => "fraction",
            (Self::Fraction, GrammaticalNumber::Plural) => "fractions",
            (Self::Bool, GrammaticalNumber::Singular) => "bool",
            (Self::Bool, GrammaticalNumber::Plural) => "bools",
            (Self::Char, GrammaticalNumber::Singular) => "char",
            (Self::Char, GrammaticalNumber::Plural) => "chars",
            (Self::Tag, GrammaticalNumber::Singular) => "tag",
            (Self::Tag, GrammaticalNumber::Plural) => "tags",
            (Self::String, GrammaticalNumber::Singular) => "string",
            (Self::String, GrammaticalNumber::Plural) => "strings",
            (Self::List, GrammaticalNumber::Singular) => "list",
            (Self::List, GrammaticalNumber::Plural) => "lists",
            (Self::Dict, GrammaticalNumber::Singular) => "dict",
            (Self::Dict, GrammaticalNumber::Plural) => "dicts",
            #[cfg(not(target_arch = "wasm32"))]
            (Self::Stream, GrammaticalNumber::Singular) => "stream",
            #[cfg(not(target_arch = "wasm32"))]
            (Self::Stream, GrammaticalNumber::Plural) => "streams",
        }
    }
}

impl ClassName {
    const fn render(self, number: GrammaticalNumber) -> &'static str {
        match (self, number) {
            (Self::Number, GrammaticalNumber::Singular) => "number",
            (Self::Number, GrammaticalNumber::Plural) => "numbers",
            (Self::RealNumber, GrammaticalNumber::Singular) => "real number",
            (Self::RealNumber, GrammaticalNumber::Plural) => "real numbers",
            (Self::Atom, GrammaticalNumber::Singular) => "atom",
            (Self::Atom, GrammaticalNumber::Plural) => "atoms",
            (Self::Callable, GrammaticalNumber::Singular) => "callable",
            (Self::Callable, GrammaticalNumber::Plural) => "callables",
            (Self::Pair, GrammaticalNumber::Singular) => "pair",
            (Self::Pair, GrammaticalNumber::Plural) => "pairs",
        }
    }
}

impl Modifier {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Positive => "positive",
            Self::NonNegative => "non-negative",
            Self::Finite => "finite",
        }
    }
}

fn render_alternatives(alternatives: impl IntoIterator<Item = String>) -> String {
    let alternatives = alternatives.into_iter().collect::<Vec<_>>();
    match alternatives.as_slice() {
        [] => "value".to_string(),
        [only] => only.clone(),
        [first, second] => format!("{first} or {second}"),
        [head @ .., last] => format!("{}, or {last}", head.join(", ")),
    }
}

fn render_int_range(lower: Bound, upper: Bound, number: GrammaticalNumber) -> String {
    let noun = TypeName::Int.render(number);
    match (lower, upper) {
        (Bound::Unbounded, Bound::Unbounded) => noun.to_string(),
        (Bound::Included(lower), Bound::Unbounded) => format!("{noun} at least {lower}"),
        (Bound::Excluded(lower), Bound::Unbounded) => {
            format!("{noun} greater than {lower}")
        }
        (Bound::Unbounded, Bound::Included(upper)) => format!("{noun} at most {upper}"),
        (Bound::Unbounded, Bound::Excluded(upper)) => format!("{noun} less than {upper}"),
        (Bound::Included(lower), Bound::Included(upper)) => {
            format!("{noun} from {lower} through {upper}")
        }
        (Bound::Excluded(lower), Bound::Included(upper)) => {
            format!("{noun} greater than {lower} and at most {upper}")
        }
        (Bound::Included(lower), Bound::Excluded(upper)) => {
            format!("{noun} at least {lower} and less than {upper}")
        }
        (Bound::Excluded(lower), Bound::Excluded(upper)) => {
            format!("{noun} greater than {lower} and less than {upper}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_atomic_types_and_classes() {
        assert_eq!(Requirement::INT.to_string(), "int");
        assert_eq!(Requirement::NUMBER.to_string(), "number");
        assert_eq!(Requirement::REAL_NUMBER.to_string(), "real number");
        assert_eq!(Requirement::CALLABLE.to_string(), "callable");
    }

    #[test]
    fn renders_unions_with_consistent_punctuation() {
        assert_eq!(
            Requirement::one_of([Requirement::INT, Requirement::FLOAT]).to_string(),
            "int or float"
        );
        assert_eq!(
            Requirement::one_of([Requirement::INT, Requirement::FLOAT, Requirement::FRACTION,])
                .to_string(),
            "int, float, or fraction"
        );
        assert_eq!(
            Requirement::one_of([
                Requirement::INT,
                Requirement::one_of([Requirement::FLOAT, Requirement::INT]),
            ])
            .to_string(),
            "int or float"
        );
    }

    #[test]
    fn renders_nested_containers_in_public_prose() {
        assert_eq!(
            Requirement::list(Requirement::INT).to_string(),
            "list of ints"
        );
        assert_eq!(
            Requirement::dict(Requirement::TAG, Requirement::INT).to_string(),
            "dict with tag keys and int values"
        );
        assert_eq!(
            Requirement::list(Requirement::one_of(
                [Requirement::INT, Requirement::STRING,]
            ))
            .to_string(),
            "list whose elements are ints or strings"
        );
        assert_eq!(
            Requirement::list(Requirement::positive(Requirement::one_of([
                Requirement::INT,
                Requirement::FLOAT,
            ])))
            .to_string(),
            "list whose elements are positive ints or positive floats"
        );
        assert_eq!(
            Requirement::dict(Requirement::TAG, Requirement::list(Requirement::INT)).to_string(),
            "dict with tag keys and values that are lists of ints"
        );
        assert_eq!(
            Requirement::dict(
                Requirement::list(Requirement::INT),
                Requirement::positive(Requirement::one_of([Requirement::INT, Requirement::FLOAT,])),
            )
            .to_string(),
            "dict with keys that are lists of ints and values that are positive ints or positive floats"
        );
    }

    #[test]
    fn renders_integer_bounds_without_interval_notation() {
        assert_eq!(
            Requirement::int_range(Bound::Included(0), Bound::Included(255)).to_string(),
            "int from 0 through 255"
        );
        assert_eq!(
            Requirement::int_range(Bound::Excluded(0), Bound::Included(255)).to_string(),
            "int greater than 0 and at most 255"
        );
        assert_eq!(
            Requirement::list(Requirement::int_range(
                Bound::Excluded(0),
                Bound::Included(255),
            ))
            .to_string(),
            "list of ints greater than 0 and at most 255"
        );
    }

    #[test]
    fn renders_modifiers_and_literals() {
        assert_eq!(
            Requirement::positive(Requirement::finite(Requirement::FLOAT)).to_string(),
            "positive finite float"
        );
        assert_eq!(
            Requirement::one_of([
                Requirement::non_negative(Requirement::INT),
                Requirement::literal("inf"),
            ])
            .to_string(),
            "non-negative int or inf"
        );
        assert_eq!(
            Requirement::positive(Requirement::one_of([Requirement::INT, Requirement::FLOAT,]))
                .to_string(),
            "positive int or positive float"
        );
        assert_eq!(
            Requirement::string_literal("strict\"mode").to_string(),
            "\"strict\\\"mode\""
        );
    }
}
