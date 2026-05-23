use tower_lsp::lsp_types::{
    SemanticToken, SemanticTokenModifier, SemanticTokenType, SemanticTokensLegend,
};
use wqpl::highlight::HighlightName;

const REF_CAPTURE_MODIFIER_BIT: u32 = 1 << 0;
const GLOBAL_MODIFIER_BIT: u32 = 1 << 1;
const LOCAL_MODIFIER_BIT: u32 = 1 << 2;
const PARAMETER_MODIFIER_BIT: u32 = 1 << 3;
const IMPLICIT_PARAMETER_MODIFIER_BIT: u32 = 1 << 4;
const LOOP_COUNTER_MODIFIER_BIT: u32 = 1 << 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VariableProvenance {
    Global,
    Local,
    Parameter,
    ImplicitParameter,
    LoopCounter,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VariableTokenInfo {
    pub span: (usize, usize),
    pub provenance: VariableProvenance,
    pub ref_capture: bool,
}

pub fn legend() -> SemanticTokensLegend {
    SemanticTokensLegend {
        token_types: vec![
            SemanticTokenType::COMMENT,
            SemanticTokenType::VARIABLE,
            SemanticTokenType::ENUM_MEMBER,
            SemanticTokenType::FUNCTION,
            SemanticTokenType::KEYWORD,
            SemanticTokenType::NAMESPACE,
            SemanticTokenType::NUMBER,
            SemanticTokenType::OPERATOR,
            SemanticTokenType::PROPERTY,
            SemanticTokenType::STRING,
            SemanticTokenType::TYPE,
            SemanticTokenType::PARAMETER,
        ],
        token_modifiers: vec![
            SemanticTokenModifier::new("refCapture"),
            SemanticTokenModifier::new("global"),
            SemanticTokenModifier::new("local"),
            SemanticTokenModifier::new("parameter"),
            SemanticTokenModifier::new("implicitParameter"),
            SemanticTokenModifier::new("loopCounter"),
        ],
    }
}

fn token_type_index(name: HighlightName) -> Option<u32> {
    Some(match name {
        HighlightName::Comment => 0,
        HighlightName::Variable
        | HighlightName::VariableOuter
        | HighlightName::VariableRefCapture
        | HighlightName::VariableBuiltin => 1,
        HighlightName::Constant
        | HighlightName::ConstantBuiltin
        | HighlightName::Boolean
        | HighlightName::Tag => 2,
        HighlightName::Function | HighlightName::FunctionCall | HighlightName::FunctionBuiltin => 3,
        HighlightName::Keyword | HighlightName::KeywordReturn | HighlightName::KeywordDebug => 4,
        HighlightName::Module => 5,
        HighlightName::Number => 6,
        HighlightName::Operator
        | HighlightName::OperatorPipe
        | HighlightName::PunctuationSpecial => 7,
        HighlightName::Property | HighlightName::PropertyBuiltin => 8,
        HighlightName::String | HighlightName::StringSpecial => 9,
        HighlightName::Type | HighlightName::TypeBuiltin => 10,
        HighlightName::VariableParameter => 11,
        _ => return None,
    })
}

fn byte_offset_to_position(src: &str, offset: usize) -> tower_lsp::lsp_types::Position {
    let mut line = 0u32;
    let mut line_start = 0usize;
    for (i, c) in src.char_indices() {
        if i >= offset {
            break;
        }
        if c == '\n' {
            line += 1;
            line_start = i + c.len_utf8();
        }
    }
    let line_text = &src[line_start..offset.min(src.len())];
    let character = line_text.encode_utf16().count() as u32;
    tower_lsp::lsp_types::Position { line, character }
}

pub fn semantic_tokens_from_events_with_variable_info(
    src: &str,
    events: &[wqpl::highlight::HighlightEvent],
    variable_infos: &[VariableTokenInfo],
) -> Vec<SemanticToken> {
    let mut tokens = Vec::new();
    let mut stack: Vec<HighlightName> = Vec::new();
    let mut last_line = 0u32;
    let mut last_char = 0u32;

    for event in events {
        match event {
            wqpl::highlight::HighlightEvent::HighlightStart(name) => {
                stack.push(*name);
            }
            wqpl::highlight::HighlightEvent::HighlightEnd => {
                stack.pop();
            }
            wqpl::highlight::HighlightEvent::Source { start, end } => {
                if let Some(&name) = stack.last()
                    && let Some(token_type) = token_type_index(name)
                {
                    let start_pos = byte_offset_to_position(src, *start);
                    let end_pos = byte_offset_to_position(src, *end);
                    let length = end_pos.character.saturating_sub(start_pos.character);

                    let delta_line = start_pos.line.saturating_sub(last_line);
                    let delta_start = if delta_line == 0 {
                        start_pos.character.saturating_sub(last_char)
                    } else {
                        start_pos.character
                    };

                    tokens.push(SemanticToken {
                        delta_line,
                        delta_start,
                        length,
                        token_type,
                        token_modifiers_bitset: modifiers_for_span(variable_infos, *start, *end),
                    });

                    last_line = start_pos.line;
                    last_char = start_pos.character;
                }
            }
        }
    }

    tokens
}

fn modifiers_for_span(variable_infos: &[VariableTokenInfo], start: usize, end: usize) -> u32 {
    variable_infos
        .iter()
        .filter(|info| info.span.0 < end && start < info.span.1)
        .fold(0, |bits, info| bits | modifier_bits(*info))
}

fn modifier_bits(info: VariableTokenInfo) -> u32 {
    let provenance_bit = match info.provenance {
        VariableProvenance::Global => GLOBAL_MODIFIER_BIT,
        VariableProvenance::Local => LOCAL_MODIFIER_BIT,
        VariableProvenance::Parameter => PARAMETER_MODIFIER_BIT,
        VariableProvenance::ImplicitParameter => IMPLICIT_PARAMETER_MODIFIER_BIT,
        VariableProvenance::LoopCounter => LOOP_COUNTER_MODIFIER_BIT,
    };
    if info.ref_capture {
        provenance_bit | REF_CAPTURE_MODIFIER_BIT
    } else {
        provenance_bit
    }
}

#[cfg(test)]
mod tests {
    use wqpl::highlight::{HighlightEvent, HighlightName};

    use super::*;

    #[test]
    fn test_byte_offset_to_position() {
        let src = "hello\nworld";
        assert_eq!(
            byte_offset_to_position(src, 0),
            tower_lsp::lsp_types::Position {
                line: 0,
                character: 0,
            }
        );
        assert_eq!(
            byte_offset_to_position(src, 5),
            tower_lsp::lsp_types::Position {
                line: 0,
                character: 5,
            }
        );
        assert_eq!(
            byte_offset_to_position(src, 6),
            tower_lsp::lsp_types::Position {
                line: 1,
                character: 0,
            }
        );
        assert_eq!(
            byte_offset_to_position(src, 11),
            tower_lsp::lsp_types::Position {
                line: 1,
                character: 5,
            }
        );
    }

    #[test]
    fn test_semantic_tokens_simple() {
        let src = "a 1";
        let events = vec![
            HighlightEvent::HighlightStart(HighlightName::Variable),
            HighlightEvent::Source { start: 0, end: 1 },
            HighlightEvent::HighlightEnd,
            HighlightEvent::Source { start: 1, end: 2 },
            HighlightEvent::HighlightStart(HighlightName::Number),
            HighlightEvent::Source { start: 2, end: 3 },
            HighlightEvent::HighlightEnd,
        ];
        let tokens = semantic_tokens_from_events_with_variable_info(src, &events, &[]);
        assert_eq!(tokens.len(), 2);

        // Variable "a" at line 0, char 0, length 1
        assert_eq!(tokens[0].delta_line, 0);
        assert_eq!(tokens[0].delta_start, 0);
        assert_eq!(tokens[0].length, 1);
        assert_eq!(tokens[0].token_type, 1); // VARIABLE

        // Number "1" at line 0, char 2, length 1
        assert_eq!(tokens[1].delta_line, 0);
        assert_eq!(tokens[1].delta_start, 2);
        assert_eq!(tokens[1].length, 1);
        assert_eq!(tokens[1].token_type, 6); // NUMBER
    }

    #[test]
    fn provenance_modifiers_mark_overlapping_token() {
        let src = "a 1";
        let events = vec![
            HighlightEvent::HighlightStart(HighlightName::Variable),
            HighlightEvent::Source { start: 0, end: 1 },
            HighlightEvent::HighlightEnd,
            HighlightEvent::HighlightStart(HighlightName::Number),
            HighlightEvent::Source { start: 2, end: 3 },
            HighlightEvent::HighlightEnd,
        ];
        let infos = [VariableTokenInfo {
            span: (0, 1),
            provenance: VariableProvenance::Global,
            ref_capture: true,
        }];
        let tokens = semantic_tokens_from_events_with_variable_info(src, &events, &infos);
        assert_eq!(
            tokens[0].token_modifiers_bitset,
            REF_CAPTURE_MODIFIER_BIT | GLOBAL_MODIFIER_BIT
        );
        assert_eq!(tokens[1].token_modifiers_bitset, 0);
    }

    #[test]
    fn parameter_highlight_becomes_parameter_token() {
        let src = "x";
        let events = vec![
            HighlightEvent::HighlightStart(HighlightName::VariableParameter),
            HighlightEvent::Source { start: 0, end: 1 },
            HighlightEvent::HighlightEnd,
        ];
        let infos = [VariableTokenInfo {
            span: (0, 1),
            provenance: VariableProvenance::Parameter,
            ref_capture: false,
        }];
        let tokens = semantic_tokens_from_events_with_variable_info(src, &events, &infos);

        assert_eq!(tokens[0].token_type, 11);
        assert_eq!(tokens[0].token_modifiers_bitset, PARAMETER_MODIFIER_BIT);
    }
}
