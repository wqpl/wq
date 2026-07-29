use std::sync::OnceLock;

use icu_casemap::CaseMapper;
use unicode_normalization::UnicodeNormalization as _;
use unicode_width::UnicodeWidthStr as _;

#[cfg(test)]
pub(crate) const VERSION: (u8, u8, u8) = (17, 0, 0);
pub(crate) const VERSION_STRING: &str = "17.0.0";

const NAMED_SEQUENCES_DATA: &str = include_str!("unicode/NamedSequences.txt");

#[derive(Debug)]
struct NamedSequence {
    name: &'static str,
    loose_name: String,
    value: String,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum NormalizationForm {
    Nfc,
    Nfd,
    Nfkc,
    Nfkd,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum CaseMode {
    Lower,
    Upper,
    Fold,
}

pub(crate) fn character_name(value: char) -> Option<String> {
    unicode_names2::name(value).map(|name| name.to_string())
}

pub(crate) fn named_sequence_name(value: &str) -> Option<&'static str> {
    named_sequences()
        .iter()
        .find(|entry| entry.value == value)
        .map(|entry| entry.name)
}

pub(crate) fn lookup_name(name: &str) -> Option<String> {
    if let Some(value) = unicode_names2::character(name) {
        return Some(value.to_string());
    }

    let loose_name = loose_name_key(name);
    named_sequences()
        .iter()
        .find(|entry| entry.loose_name == loose_name)
        .map(|entry| entry.value.clone())
}

pub(crate) fn normalize(value: &str, form: NormalizationForm) -> String {
    match form {
        NormalizationForm::Nfc => value.nfc().collect(),
        NormalizationForm::Nfd => value.nfd().collect(),
        NormalizationForm::Nfkc => value.nfkc().collect(),
        NormalizationForm::Nfkd => value.nfkd().collect(),
    }
}

pub(crate) fn change_case(value: &str, mode: CaseMode) -> String {
    let mapper = CaseMapper::new();
    let root = "und"
        .parse()
        .expect("Unicode root language identifier is valid");
    match mode {
        CaseMode::Lower => mapper.lowercase_to_string(value, &root).into_owned(),
        CaseMode::Upper => mapper.uppercase_to_string(value, &root).into_owned(),
        CaseMode::Fold => mapper.fold_string(value).into_owned(),
    }
}

pub(crate) fn is_whitespace(value: char) -> bool {
    matches!(
        value,
        '\u{0009}'..='\u{000d}'
            | '\u{0020}'
            | '\u{0085}'
            | '\u{00a0}'
            | '\u{1680}'
            | '\u{2000}'..='\u{200a}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{202f}'
            | '\u{205f}'
            | '\u{3000}'
    )
}

pub(crate) fn is_xid_start(value: char) -> bool {
    unicode_ident::is_xid_start(value)
}

pub(crate) fn is_xid_continue(value: char) -> bool {
    unicode_ident::is_xid_continue(value)
}

pub(crate) fn terminal_width(value: &str) -> usize {
    value.width()
}

fn named_sequences() -> &'static [NamedSequence] {
    static NAMED_SEQUENCES: OnceLock<Vec<NamedSequence>> = OnceLock::new();
    NAMED_SEQUENCES.get_or_init(|| {
        NAMED_SEQUENCES_DATA
            .lines()
            .filter_map(parse_named_sequence)
            .collect()
    })
}

fn parse_named_sequence(line: &'static str) -> Option<NamedSequence> {
    let line = line.split('#').next()?.trim();
    if line.is_empty() {
        return None;
    }
    let (name, code_points) = line.split_once(';')?;
    let name = name.trim();
    let value = code_points
        .split_ascii_whitespace()
        .map(|code_point| {
            let value =
                u32::from_str_radix(code_point, 16).expect("valid checked-in Unicode code point");
            char::from_u32(value).expect("valid checked-in Unicode scalar")
        })
        .collect();
    Some(NamedSequence {
        name,
        loose_name: loose_name_key(name),
        value,
    })
}

fn loose_name_key(name: &str) -> String {
    let chars: Vec<char> = name.chars().collect();
    let mut key = String::with_capacity(chars.len());
    for (index, value) in chars.iter().copied().enumerate() {
        if value == '_' || value.is_ascii_whitespace() {
            continue;
        }
        if value == '-'
            && index > 0
            && index + 1 < chars.len()
            && chars[index - 1].is_ascii_alphanumeric()
            && chars[index + 1].is_ascii_alphanumeric()
        {
            continue;
        }
        key.push(value.to_ascii_uppercase());
    }
    key
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dependency_versions_match_the_public_version() {
        assert_eq!(unicode_normalization::UNICODE_VERSION, VERSION);
        assert_eq!(
            unicode_segmentation::UNICODE_VERSION,
            (
                u64::from(VERSION.0),
                u64::from(VERSION.1),
                u64::from(VERSION.2)
            )
        );
        assert_eq!(unicode_width::UNICODE_VERSION, VERSION);
    }

    #[test]
    fn names_include_aliases_and_approved_sequences() {
        assert_eq!(character_name('☃').as_deref(), Some("SNOWMAN"));
        assert_eq!(lookup_name("snowman").as_deref(), Some("☃"));
        assert_eq!(lookup_name("backspace").as_deref(), Some("\u{0008}"));
        assert_eq!(
            lookup_name("keycap_digit_one").as_deref(),
            Some("1\u{fe0f}\u{20e3}")
        );
        assert_eq!(
            named_sequence_name("1\u{fe0f}\u{20e3}"),
            Some("KEYCAP DIGIT ONE")
        );
    }

    #[test]
    fn whitespace_uses_the_unicode_white_space_property() {
        assert!(is_whitespace(' '));
        assert!(is_whitespace('\u{2029}'));
        assert!(!is_whitespace('\u{180e}'));
        assert!(!is_whitespace('\u{200b}'));
    }

    #[test]
    fn xid_queries_are_raw_unicode_properties() {
        assert!(is_xid_start('λ'));
        assert!(is_xid_continue('\u{0301}'));
        assert!(!is_xid_start('1'));
    }
}
