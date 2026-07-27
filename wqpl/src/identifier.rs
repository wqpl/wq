use unicode_normalization::UnicodeNormalization;

use crate::token::TokenType;

/// Return whether `ch` can start a wq identifier or tag name.
pub fn is_identifier_start(ch: char) -> bool {
    ch == '_' || unicode_ident::is_xid_start(ch)
}

/// Return whether `ch` can continue a wq identifier or tag name.
pub fn is_identifier_continue(ch: char) -> bool {
    ch == '_' || ch == '?' || unicode_ident::is_xid_continue(ch)
}

/// Return whether `name` has the lexical shape of a wq identifier.
pub fn is_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(is_identifier_start) && chars.all(is_identifier_continue)
}

/// Normalize an identifier or tag name to Unicode NFC.
pub fn normalize_identifier(name: &str) -> String {
    name.nfc().collect()
}

pub(crate) fn is_bindable_identifier(name: &str) -> bool {
    is_identifier(name) && !TokenType::is_reserved_identifier(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifier_requires_a_start_character() {
        assert!(is_identifier("a"));
        assert!(is_identifier("_"));
        assert!(is_identifier("λ"));
        assert!(is_identifier("a?"));
        assert!(is_identifier("e\u{301}"));
        assert!(!is_identifier(""));
        assert!(!is_identifier("?a"));
        assert!(!is_identifier("1a"));
        assert!(!is_identifier("\u{301}a"));
        assert!(!is_identifier("\u{37a}"));
    }

    #[test]
    fn bindable_identifiers_exclude_reserved_names() {
        assert!(is_bindable_identifier("value"));
        assert!(is_bindable_identifier("value?"));
        assert!(is_bindable_identifier("_"));
        assert!(!is_bindable_identifier("T"));
        assert!(!is_bindable_identifier("and"));
    }

    #[test]
    fn identifiers_normalize_to_nfc() {
        assert_eq!(normalize_identifier("e\u{301}"), "é");
        assert_eq!(normalize_identifier("각"), "각");
        assert_eq!(normalize_identifier("각"), "각");
    }
}
