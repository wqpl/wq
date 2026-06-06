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
        advance(lexer);
        if (!lexer->eof(lexer)) {
          advance(lexer);
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
      advance(lexer);
      if (!lexer->eof(lexer)) {
        advance(lexer);
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

static void skip_string_in_format_expr(TSLexer *lexer) {
  scan_quoted_string(lexer);
}

static void skip_format_expr(TSLexer *lexer) {
  unsigned depth = 1;
  while (depth > 0 && !lexer->eof(lexer)) {
    if (lexer->lookahead == '"') {
      skip_string_in_format_expr(lexer);
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
      advance(lexer);
      if (!lexer->eof(lexer)) {
        advance(lexer);
      }
      continue;
    }

    if (lexer->lookahead == '{') {
      advance(lexer);
      if (lexer->lookahead == '{') {
        advance(lexer);
      } else {
        skip_format_expr(lexer);
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
