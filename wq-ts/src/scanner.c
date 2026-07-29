#include "tree_sitter/parser.h"

#include <stdbool.h>

enum TokenType {
  BLOCK_COMMENT,
  STRING_CONTENT,
  RAW_STRING_CONTENT,
  FORMAT_STRING_START,
  FORMAT_STRING_END,
  FORMAT_SPEC,
  INDENTED_NEWLINE,
  DIRECTIVE,
  SHEBANG,
};

static inline void advance(TSLexer *lexer) {
  lexer->advance(lexer, false);
}

static inline void skip(TSLexer *lexer) {
  lexer->advance(lexer, true);
}

static bool is_hex_digit(int32_t ch) {
  return
    (ch >= '0' && ch <= '9') ||
    (ch >= 'a' && ch <= 'f') ||
    (ch >= 'A' && ch <= 'F');
}

static uint32_t hex_value(int32_t ch) {
  if (ch >= '0' && ch <= '9') {
    return (uint32_t)(ch - '0');
  }
  if (ch >= 'a' && ch <= 'f') {
    return (uint32_t)(ch - 'a' + 10);
  }
  return (uint32_t)(ch - 'A' + 10);
}

static bool is_inline_whitespace(int32_t ch) {
  return
    ch == ' ' ||
    ch == '\t' ||
    ch == '\r' ||
    ch == '\f' ||
    ch == '\v' ||
    ch == 0x0085 ||
    ch == 0x00a0 ||
    ch == 0x1680 ||
    (ch >= 0x2000 && ch <= 0x200a) ||
    ch == 0x2028 ||
    ch == 0x2029 ||
    ch == 0x202f ||
    ch == 0x205f ||
    ch == 0x3000;
}

static bool scan_escape(TSLexer *lexer) {
  advance(lexer);
  if (lexer->eof(lexer)) {
    return false;
  }

  int32_t kind = lexer->lookahead;
  advance(lexer);
  if (kind == 'x') {
    for (unsigned i = 0; i < 2; i++) {
      if (!is_hex_digit(lexer->lookahead)) {
        return false;
      }
      advance(lexer);
    }
    return true;
  }

  if (kind == 'N') {
    if (lexer->lookahead != '{') {
      return false;
    }
    advance(lexer);
    unsigned characters = 0;
    while (!lexer->eof(lexer) && lexer->lookahead != '}') {
      if (lexer->lookahead == '\n' || lexer->lookahead == '\r') {
        return false;
      }
      characters++;
      advance(lexer);
    }
    if (characters == 0 || lexer->lookahead != '}') {
      return false;
    }
    advance(lexer);
    return true;
  }

  if (kind == 'U') {
    return false;
  }

  if (kind != 'u') {
    return true;
  }
  if (lexer->lookahead != '{') {
    return false;
  }
  advance(lexer);

  uint32_t value = 0;
  unsigned digits = 0;
  while (is_hex_digit(lexer->lookahead)) {
    if (digits == 6) {
      return false;
    }
    value = (value << 4) | hex_value(lexer->lookahead);
    digits++;
    advance(lexer);
  }
  if (digits == 0 || lexer->lookahead != '}') {
    return false;
  }
  advance(lexer);
  return value <= 0x10ffff && !(value >= 0xd800 && value <= 0xdfff);
}

static void skip_inline_whitespace(TSLexer *lexer) {
  while (is_inline_whitespace(lexer->lookahead)) {
    skip(lexer);
  }
}

static bool scan_block_comment(TSLexer *lexer) {
  if (lexer->lookahead != '/') {
    return false;
  }
  advance(lexer);
  if (lexer->lookahead != '*') {
    return false;
  }
  advance(lexer);

  unsigned depth = 1;
  while (depth > 0) {
    if (lexer->eof(lexer)) {
      return false;
    }

    if (lexer->lookahead == '/') {
      advance(lexer);
      if (lexer->lookahead == '*') {
        advance(lexer);
        depth++;
      }
      continue;
    }

    if (lexer->lookahead == '*') {
      advance(lexer);
      if (lexer->lookahead == '/') {
        advance(lexer);
        depth--;
      }
      continue;
    }

    advance(lexer);
  }

  lexer->mark_end(lexer);
  return true;
}

static bool scan_quoted_string(TSLexer *lexer) {
  if (lexer->lookahead != '"') {
    return false;
  }

  unsigned quote_count = 0;
  while (lexer->lookahead == '"') {
    advance(lexer);
    quote_count++;
  }

  if (quote_count == 2) {
    lexer->mark_end(lexer);
    return true;
  }

  if (quote_count == 1) {
    while (!lexer->eof(lexer)) {
      if (lexer->lookahead == '\\') {
        if (!scan_escape(lexer)) {
          return false;
        }
        continue;
      }
      if (lexer->lookahead == '"') {
        advance(lexer);
        lexer->mark_end(lexer);
        return true;
      }
      advance(lexer);
    }
    return false;
  }

  unsigned consecutive_quotes = 0;
  while (!lexer->eof(lexer)) {
    if (lexer->lookahead == '"') {
      advance(lexer);
      consecutive_quotes++;
      if (consecutive_quotes == quote_count) {
        lexer->mark_end(lexer);
        return true;
      }
      continue;
    }

    consecutive_quotes = 0;
    if (lexer->lookahead == '\\') {
      if (!scan_escape(lexer)) {
        return false;
      }
      continue;
    }
    advance(lexer);
  }

  return false;
}

static bool scan_raw_string(TSLexer *lexer) {
  if (lexer->lookahead != '"') {
    return false;
  }

  advance(lexer);
  while (!lexer->eof(lexer)) {
    if (lexer->lookahead == '"') {
      advance(lexer);
      lexer->mark_end(lexer);
      return true;
    }
    advance(lexer);
  }

  return false;
}

static bool scan_format_spec(TSLexer *lexer) {
  if (lexer->lookahead != '[') {
    return false;
  }
  advance(lexer);

  unsigned brace_depth = 0;
  while (!lexer->eof(lexer)) {
    if (lexer->lookahead == '"') {
      if (!scan_quoted_string(lexer)) {
        return false;
      }
      continue;
    }
    if (lexer->lookahead == '{') {
      brace_depth++;
      advance(lexer);
      continue;
    }
    if (lexer->lookahead == '}') {
      if (brace_depth > 0) {
        brace_depth--;
      }
      advance(lexer);
      continue;
    }
    if (lexer->lookahead == ']' && brace_depth == 0) {
      advance(lexer);
      lexer->mark_end(lexer);
      return true;
    }
    advance(lexer);
  }
  return false;
}

static bool scan_format_quote(TSLexer *lexer) {
  if (lexer->lookahead != '"') {
    return false;
  }
  advance(lexer);
  lexer->mark_end(lexer);
  return true;
}

static bool scan_indented_newline(TSLexer *lexer) {
  if (lexer->lookahead != '\n') {
    return false;
  }

  bool indented = false;
  do {
    advance(lexer);
    indented = false;
    while (is_inline_whitespace(lexer->lookahead)) {
      advance(lexer);
      indented = true;
    }
  } while (lexer->lookahead == '\n');

  if (!indented || lexer->eof(lexer)) {
    return false;
  }
  lexer->mark_end(lexer);
  return true;
}

static bool scan_shebang(TSLexer *lexer) {
  if (lexer->get_column(lexer) != 0 || lexer->lookahead != '#') {
    return false;
  }
  advance(lexer);
  if (lexer->lookahead != '!') {
    return false;
  }
  advance(lexer);
  while (!lexer->eof(lexer) && lexer->lookahead != '\n') {
    advance(lexer);
  }
  lexer->mark_end(lexer);
  return true;
}

static bool scan_directive(TSLexer *lexer) {
  if (lexer->get_column(lexer) != 0) {
    return false;
  }
  while (is_inline_whitespace(lexer->lookahead)) {
    advance(lexer);
  }
  if (lexer->lookahead != '\\') {
    return false;
  }
  advance(lexer);
  while (!lexer->eof(lexer) && lexer->lookahead != '\n') {
    advance(lexer);
  }
  lexer->mark_end(lexer);
  return true;
}

void *tree_sitter_wq_external_scanner_create(void) {
  return NULL;
}

void tree_sitter_wq_external_scanner_destroy(void *payload) {
  (void)payload;
}

unsigned tree_sitter_wq_external_scanner_serialize(void *payload, char *buffer) {
  (void)payload;
  (void)buffer;
  return 0;
}

void tree_sitter_wq_external_scanner_deserialize(
  void *payload,
  const char *buffer,
  unsigned length
) {
  (void)payload;
  (void)buffer;
  (void)length;
}

bool tree_sitter_wq_external_scanner_scan(
  void *payload,
  TSLexer *lexer,
  const bool *valid_symbols
) {
  (void)payload;

  if (
    valid_symbols[SHEBANG] &&
    lexer->get_column(lexer) == 0 &&
    lexer->lookahead == '#'
  ) {
    lexer->result_symbol = SHEBANG;
    return scan_shebang(lexer);
  }

  if (
    valid_symbols[DIRECTIVE] &&
    lexer->get_column(lexer) == 0 &&
    (lexer->lookahead == '\\' || is_inline_whitespace(lexer->lookahead))
  ) {
    lexer->result_symbol = DIRECTIVE;
    return scan_directive(lexer);
  }

  skip_inline_whitespace(lexer);

  if (valid_symbols[BLOCK_COMMENT] && lexer->lookahead == '/') {
    lexer->result_symbol = BLOCK_COMMENT;
    return scan_block_comment(lexer);
  }

  if (valid_symbols[STRING_CONTENT] && lexer->lookahead == '"') {
    lexer->result_symbol = STRING_CONTENT;
    return scan_quoted_string(lexer);
  }

  if (valid_symbols[RAW_STRING_CONTENT] && lexer->lookahead == '"') {
    lexer->result_symbol = RAW_STRING_CONTENT;
    return scan_raw_string(lexer);
  }

  if (valid_symbols[FORMAT_STRING_START] && lexer->lookahead == '"') {
    lexer->result_symbol = FORMAT_STRING_START;
    return scan_format_quote(lexer);
  }

  if (valid_symbols[FORMAT_STRING_END] && lexer->lookahead == '"') {
    lexer->result_symbol = FORMAT_STRING_END;
    return scan_format_quote(lexer);
  }

  if (valid_symbols[FORMAT_SPEC] && lexer->lookahead == '[') {
    lexer->result_symbol = FORMAT_SPEC;
    return scan_format_spec(lexer);
  }

  if (valid_symbols[INDENTED_NEWLINE] && lexer->lookahead == '\n') {
    lexer->result_symbol = INDENTED_NEWLINE;
    return scan_indented_newline(lexer);
  }

  return false;
}
