#include "tree_sitter/parser.h"

#include <stdbool.h>

enum TokenType {
  BLOCK_COMMENT,
  STRING_CONTENT,
  RAW_STRING_CONTENT,
  FORMAT_STRING_CONTENT,
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

static bool scan_escape(TSLexer *lexer) {
  advance(lexer);
  if (lexer->eof(lexer)) {
    return true;
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
  while (
    lexer->lookahead == ' ' ||
    lexer->lookahead == '\t' ||
    lexer->lookahead == '\r'
  ) {
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
      lexer->mark_end(lexer);
      return true;
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
    lexer->mark_end(lexer);
    return true;
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

  lexer->mark_end(lexer);
  return true;
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

  lexer->mark_end(lexer);
  return true;
}

static bool skip_string_in_format_expr(TSLexer *lexer) {
  return scan_quoted_string(lexer);
}

static bool skip_format_expr(TSLexer *lexer) {
  unsigned depth = 1;
  while (depth > 0 && !lexer->eof(lexer)) {
    if (lexer->lookahead == '"') {
      if (!skip_string_in_format_expr(lexer)) {
        return false;
      }
      continue;
    }

    if (lexer->lookahead == '{') {
      advance(lexer);
      depth++;
      continue;
    }

    if (lexer->lookahead == '}') {
      advance(lexer);
      depth--;
      continue;
    }

    advance(lexer);
  }
  return depth == 0;
}

static bool scan_format_string(TSLexer *lexer) {
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

    if (lexer->lookahead == '\\') {
      if (!scan_escape(lexer)) {
        return false;
      }
      continue;
    }

    if (lexer->lookahead == '{') {
      advance(lexer);
      if (lexer->lookahead == '{') {
        advance(lexer);
      } else {
        if (!skip_format_expr(lexer)) {
          return false;
        }
      }
      continue;
    }

    if (lexer->lookahead == '}') {
      advance(lexer);
      if (lexer->lookahead == '}') {
        advance(lexer);
      }
      continue;
    }

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

  skip_inline_whitespace(lexer);

  if (valid_symbols[BLOCK_COMMENT]) {
    lexer->result_symbol = BLOCK_COMMENT;
    if (scan_block_comment(lexer)) {
      return true;
    }
  }

  if (valid_symbols[STRING_CONTENT]) {
    lexer->result_symbol = STRING_CONTENT;
    if (scan_quoted_string(lexer)) {
      return true;
    }
  }

  if (valid_symbols[RAW_STRING_CONTENT]) {
    lexer->result_symbol = RAW_STRING_CONTENT;
    if (scan_raw_string(lexer)) {
      return true;
    }
  }

  if (valid_symbols[FORMAT_STRING_CONTENT]) {
    lexer->result_symbol = FORMAT_STRING_CONTENT;
    if (scan_format_string(lexer)) {
      return true;
    }
  }

  return false;
}
