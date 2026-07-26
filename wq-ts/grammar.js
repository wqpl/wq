const PREC = {
  ASSIGN: 1,
  PIPE: 2,
  COMMA: 3,
  COMPARE: 4,
  ADD: 5,
  MULTIPLY: 6,
  RANGE: 7,
  UNARY: 8,
  POWER: 9,
  POSTFIX: 10,
};

const IDENT_START = /[\p{ID_Start}_]/u;
const IDENT_CONTINUE = /[\p{ID_Continue}_?]/u;
const IDENT = /[\p{ID_Start}_][\p{ID_Continue}_?]*/u;
const DEC = /[0-9](?:_?[0-9])*/;
const BIN = /[01](?:_?[01])*/;
const OCT = /[0-7](?:_?[0-7])*/;
const HEX = /[0-9a-fA-F](?:_?[0-9a-fA-F])*/;

// Decimal floats require digits on both sides of the dot. A standalone dot
// has no token rule, so malformed inputs such as `.1` produce an ERROR node.
const decimalFloat = choice(
  seq(DEC, ".", DEC, optional(seq(/[eE]/, optional(/[+-]/), DEC))),
  seq(DEC, /[eE]/, optional(/[+-]/), DEC),
);

const integerBody = choice(
  seq(/0[bB]/, BIN),
  seq(/0[oO]/, OCT),
  seq(/0[xX]/, HEX),
  DEC,
);

const separated1 = (rule, sep) => seq(rule, repeat(seq(sep, rule)));

export default grammar({
  name: "wq",

  extras: ($) => [/[ \t\r]/, $.comment],

  externals: ($) => [
    $.block_comment,
    $._string_content, // scanner validates quoted escape syntax
    $._raw_string_content,
    $._format_string_content,
  ],

  word: ($) => $.identifier,

  reserved: {
    global: (_) => [
      "W",
      "N",
      "B",
      "A",
      "and",
      "O",
      "or",
      "T",
      "true",
      "F",
      "false",
      "inf",
    ],
  },

  conflicts: ($) => [
    [$.literal, $.dict_pair],
    [$.operator_identifier, $.unary_expr],
    [$._non_comma_operator_identifier, $._non_terminator_unary_expr],
    [$._leading_comma_expr, $.operator_identifier],
    [$.pause_form],
  ],

  rules: {
    source_file: ($) =>
      seq(optional($.shebang), optional($._statement_sequence)),

    statement: ($) => choice($.magic_command, $.expression),

    separator: ($) => choice(";", $.newline),

    block: ($) => $._statement_sequence,

    _statement_sequence: ($) =>
      choice(
        repeat1($.separator),
        seq(
          repeat($.separator),
          $.statement,
          repeat(seq(repeat1($.separator), $.statement)),
          repeat($.separator),
        ),
      ),

    expression: ($) => $.assignment_expr,

    assignment_expr: ($) =>
      choice(
        prec.right(
          PREC.ASSIGN,
          seq(
            field("left", $.pipe_expr),
            field("operator", $.assignment_operator),
            field("right", $.assignment_expr),
          ),
        ),
        $.pipe_expr,
      ),

    assignment_operator: (_) =>
      choice(
        ":",
        "+:",
        "-:",
        "*:",
        "/:",
        "/.:",
        "%:",
        "^:",
        "^.:",
        ",:",
        "/%:",
      ),

    pipe_expr: ($) =>
      choice(
        prec.left(
          PREC.PIPE,
          seq(
            $.comma_expr,
            repeat1(seq(field("operator", $.pipe_operator), $.pipe_rhs_expr)),
          ),
        ),
        $.comma_expr,
      ),

    pipe_operator: (_) => choice("||.", "||", "|.", "|"),

    pipe_rhs_expr: ($) =>
      choice($.pipe_checkpoint_assignment, $.pipe_postfix_expr),

    pipe_checkpoint_assignment: ($) =>
      prec.right(
        PREC.ASSIGN,
        seq(
          field("left", $.pipe_postfix_expr),
          field("operator", $.assignment_operator),
        ),
      ),

    pipe_postfix_expr: ($) =>
      choice(
        $.primary,
        prec.left(PREC.POSTFIX, seq($.pipe_postfix_expr, $.pipe_suffix)),
      ),

    pipe_suffix: ($) =>
      choice(
        $.call_suffix,
        $.mutating_index_suffix,
        $.pipe_juxtaposition_suffix,
      ),

    pipe_juxtaposition_suffix: ($) =>
      prec(PREC.POSTFIX, seq(optional($.depth_modifier), $.comparison_expr)),

    _leading_comma_expr: ($) =>
      prec(
        PREC.COMMA,
        repeat1(
          seq(
            ",",
            continuation($, alias($._leading_comma_item, $.comparison_expr)),
          ),
        ),
      ),

    _leading_comma_operand: ($) => alias($._leading_comma_expr, $.comma_expr),

    _comma_or_additive_expr: ($) =>
      choice($._leading_comma_operand, $.additive_expr),

    _comma_or_multiplicative_expr: ($) =>
      choice($._leading_comma_operand, $.multiplicative_expr),

    _comma_or_range_expr: ($) => choice($._leading_comma_operand, $.range_expr),

    _comma_or_unary_expr: ($) => choice($._leading_comma_operand, $.unary_expr),

    _comma_or_power_expr: ($) => choice($._leading_comma_operand, $.power_expr),

    _leading_comma_item: ($) => $._non_terminator_comparison_expr,

    _non_terminator_comparison_expr: ($) =>
      choice(
        prec.left(
          PREC.COMPARE,
          seq(
            $._non_terminator_additive_expr,
            repeat1(
              seq(
                field("operator", $.comparison_operator),
                continuation($, $._comma_or_additive_expr),
              ),
            ),
          ),
        ),
        $._non_terminator_additive_expr,
      ),

    _non_terminator_additive_expr: ($) =>
      binary(
        $,
        $._non_terminator_multiplicative_expr,
        $._comma_or_multiplicative_expr,
        PREC.ADD,
        choice("+", "-"),
      ),

    _non_terminator_multiplicative_expr: ($) =>
      binary(
        $,
        $._non_terminator_range_expr,
        $._comma_or_range_expr,
        PREC.MULTIPLY,
        choice("**", "/%", "/.", "*", "/", "%"),
      ),

    _non_terminator_range_expr: ($) =>
      choice(
        prec.right(
          PREC.RANGE,
          seq(
            $._non_terminator_unary_expr,
            field("operator", ".."),
            $._comma_or_unary_expr,
            field("final_operator", choice("..=", "..")),
            $._comma_or_unary_expr,
          ),
        ),
        prec.right(
          PREC.RANGE,
          seq(
            $._non_terminator_unary_expr,
            field("operator", choice("..=", "..")),
            $._comma_or_unary_expr,
          ),
        ),
        $._non_terminator_unary_expr,
      ),

    _non_terminator_unary_expr: ($) =>
      choice(
        prec(
          PREC.UNARY,
          seq(repeat1(choice("-", "#", "~")), $._comma_or_power_expr),
        ),
        $._non_terminator_power_expr,
      ),

    _non_terminator_power_expr: ($) =>
      choice(
        prec.right(
          PREC.POWER,
          seq(
            $._non_terminator_postfix_expr,
            field("operator", choice("^.", "^")),
            continuation($, $._comma_or_unary_expr),
          ),
        ),
        $._non_terminator_postfix_expr,
      ),

    _non_terminator_postfix_expr: ($) =>
      choice(
        $._non_terminator_primary,
        prec.left(
          PREC.POSTFIX,
          seq($._non_terminator_postfix_expr, $.suffix),
        ),
      ),

    _non_terminator_primary: ($) =>
      choice(
        $.literal,
        $.ellipsis,
        $.outer_variable,
        $.variable_ref,
        alias($._non_comma_operator_identifier, $.operator_identifier),
        $.function_literal,
        $.paren_expr,
        $.conditional,
        $.conditional_dot,
        $.conditional_chain,
        $.lazy_bool_form,
        $.w_loop,
        $.n_loop,
        $.return_form,
        $.break_form,
        $.continue_form,
        $.try_form,
        $.debug_form,
        $.pause_form,
        $.symbolic_form,
        $.import_form,
      ),

    comma_expr: ($) =>
      choice(
        prec.left(
          PREC.COMMA,
          seq(
            $.comparison_expr,
            repeat1(seq(",", continuation($, $.comparison_expr))),
          ),
        ),
        $._leading_comma_expr,
        $.comparison_expr,
      ),

    comparison_expr: ($) =>
      choice(
        prec.left(
          PREC.COMPARE,
          seq(
            $.additive_expr,
            repeat1(
              seq(
                field("operator", $.comparison_operator),
                continuation($, $._comma_or_additive_expr),
              ),
            ),
          ),
        ),
        $.additive_expr,
      ),

    comparison_operator: (_) =>
      choice("=.", "=", "~.", "~", "<=", "<", ">=", ">"),

    additive_expr: ($) =>
      binary(
        $,
        $.multiplicative_expr,
        $._comma_or_multiplicative_expr,
        PREC.ADD,
        choice("+", "-"),
      ),

    multiplicative_expr: ($) =>
      binary(
        $,
        $.range_expr,
        $._comma_or_range_expr,
        PREC.MULTIPLY,
        choice("**", "/%", "/.", "*", "/", "%"),
      ),

    range_expr: ($) =>
      choice(
        prec.right(
          PREC.RANGE,
          seq(
            $.unary_expr,
            field("operator", ".."),
            $._comma_or_unary_expr,
            field("final_operator", choice("..=", "..")),
            $._comma_or_unary_expr,
          ),
        ),
        prec.right(
          PREC.RANGE,
          seq(
            $.unary_expr,
            field("operator", choice("..=", "..")),
            $._comma_or_unary_expr,
          ),
        ),
        $.unary_expr,
      ),

    unary_expr: ($) =>
      choice(
        prec(
          PREC.UNARY,
          seq(repeat1(choice("-", "#", "~")), $._comma_or_power_expr),
        ),
        $.power_expr,
      ),

    power_expr: ($) =>
      choice(
        prec.right(
          PREC.POWER,
          seq(
            $.postfix_expr,
            field("operator", choice("^.", "^")),
            continuation($, $._comma_or_unary_expr),
          ),
        ),
        $.postfix_expr,
      ),

    postfix_expr: ($) =>
      choice($.primary, prec.left(PREC.POSTFIX, seq($.postfix_expr, $.suffix))),

    suffix: ($) =>
      choice($.call_suffix, $.mutating_index_suffix, $.juxtaposition_suffix),

    call_suffix: ($) =>
      prec(PREC.POSTFIX, seq(optional($.depth_modifier), $.arg_list)),

    mutating_index_suffix: ($) =>
      prec(
        PREC.POSTFIX,
        choice(seq("[", "!", optional($.argument_items), "]"), "!"),
      ),

    juxtaposition_suffix: ($) =>
      prec(PREC.POSTFIX, seq(optional($.depth_modifier), $.juxtaposition_arg)),

    juxtaposition_arg: ($) => $._juxtaposition_range_expr,

    _juxtaposition_range_expr: ($) =>
      choice(
        prec.right(
          PREC.RANGE,
          seq(
            $._juxtaposition_unary_expr,
            field("operator", ".."),
            $.unary_expr,
            field("final_operator", choice("..=", "..")),
            $.unary_expr,
          ),
        ),
        prec.right(
          PREC.RANGE,
          seq(
            $._juxtaposition_unary_expr,
            field("operator", choice("..=", "..")),
            $.unary_expr,
          ),
        ),
        $._juxtaposition_unary_expr,
      ),

    _juxtaposition_unary_expr: ($) =>
      choice(
        prec(PREC.UNARY, seq(repeat1("#"), $._juxtaposition_power_expr)),
        $._juxtaposition_power_expr,
      ),

    _juxtaposition_power_expr: ($) =>
      choice(
        prec.right(
          PREC.POWER,
          seq(
            $._juxtaposition_postfix_expr,
            field("operator", choice("^.", "^")),
            continuation($, $.unary_expr),
          ),
        ),
        $._juxtaposition_postfix_expr,
      ),

    _juxtaposition_postfix_expr: ($) =>
      choice(
        $._juxtaposition_primary,
        prec.left(
          PREC.POSTFIX,
          seq($._juxtaposition_postfix_expr, $.suffix),
        ),
      ),

    _juxtaposition_primary: ($) =>
      choice(
        $.literal,
        $.outer_variable,
        $.variable_ref,
        $.function_literal,
        $.paren_expr,
        $.conditional,
        $.conditional_dot,
        $.conditional_chain,
        $.lazy_bool_form,
        $.w_loop,
        $.n_loop,
        $.block_form,
        $.return_form,
        $.break_form,
        $.continue_form,
        $.try_form,
        $.debug_form,
        $.pause_form,
        $.symbolic_form,
        $.import_form,
      ),

    depth_modifier: (_) => token(seq("@", /[0-9](?:_?[0-9])*/)),

    arg_list: ($) => prec(2, seq("[", optional($.argument_items), "]")),

    argument_items: ($) =>
      prec(
        2,
        choice(
          $._item_separator,
          seq(
            $.expression,
            repeat(seq($._item_separator, $.expression)),
            optional($._item_separator),
          ),
        ),
      ),

    _item_separator: ($) =>
      prec.right(
        choice(
          seq(optional(repeat1($.newline)), ";", optional(repeat1($.newline))),
          repeat1($.newline),
        ),
      ),

    primary: ($) =>
      choice(
        $.literal,
        $.ellipsis,
        $.outer_variable,
        $.variable_ref,
        $.operator_identifier,
        $.function_literal,
        $.paren_expr,
        $.conditional,
        $.conditional_dot,
        $.conditional_chain,
        $.lazy_bool_form,
        $.w_loop,
        $.n_loop,
        $.block_form,
        $.return_form,
        $.break_form,
        $.continue_form,
        $.try_form,
        $.debug_form,
        $.pause_form,
        $.symbolic_form,
        $.import_form,
      ),

    literal: ($) =>
      choice(
        $.imaginary,
        $.float,
        $.integer,
        $.unicode_scalar,
        $.string,
        $.raw_string,
        $.format_string,
        $.tag,
        $.true,
        $.false,
        $.inf,
      ),

    variable_ref: ($) => $.identifier,

    outer_variable: ($) => seq("'", $.identifier),

    operator_identifier: ($) =>
      choice(
        "+",
        "-",
        "*",
        "**",
        "/",
        "/.",
        "/%",
        "%",
        "^",
        "^.",
        "=",
        "=.",
        "~",
        "~.",
        "<",
        "<=",
        ">",
        ">=",
        ",",
        "#",
      ),

    _non_comma_operator_identifier: (_) =>
      choice(
        "+",
        "-",
        "*",
        "**",
        "/",
        "/.",
        "/%",
        "%",
        "^",
        "^.",
        "=",
        "=.",
        "~",
        "~.",
        "<",
        "<=",
        ">",
        ">=",
        "#",
      ),

    lazy_bool_form: ($) =>
      prec.dynamic(1, seq(choice("A", "and", "O", "or"), $.arg_list)),

    function_literal: ($) =>
      seq(optional("'"), "{", optional($.param_list), optional($.block), "}"),

    param_list: ($) =>
      prec(
        2,
        seq(
          "[",
          optional(separated1($.param, $._param_separator)),
          optional($._param_separator),
          "]",
        ),
      ),

    _param_separator: ($) =>
      prec.right(
        choice(
          seq(optional(repeat1($.newline)), ";", optional(repeat1($.newline))),
          repeat1($.newline),
        ),
      ),

    param: ($) =>
      prec(2, choice($.identifier, seq($.tag, optional(seq(":", $.pipe_expr))))),

    paren_expr: ($) => choice($.dict_literal, $.paren_list),

    paren_list: ($) =>
      seq(
        "(",
        optional(repeat1($.newline)),
        optional(
          seq(
            $.expression,
            repeat(seq($._item_separator, $.expression)),
            optional($._item_separator),
          ),
        ),
        ")",
      ),

    dict_literal: ($) =>
      seq(
        "(",
        optional(repeat1($.newline)),
        choice(
          seq($.backtick, optional(repeat1($.newline)), ")"),
          seq($.dict_items, ")"),
        ),
      ),

    dict_items: ($) =>
      seq(
        $.dict_pair,
        repeat(seq($._item_separator, $.dict_pair)),
        optional($._item_separator),
      ),

    dict_pair: ($) => seq($.tag, ":", $.expression),

    conditional: ($) => seq("$", $.arg_list),
    conditional_dot: ($) => seq("$.", $.arg_list),
    conditional_chain: ($) => seq("$$", $.arg_list),
    w_loop: ($) => prec.dynamic(1, seq("W", $.arg_list)),
    n_loop: ($) => prec.dynamic(1, seq("N", $.arg_list)),
    block_form: ($) =>
      prec.dynamic(1, choice(seq("B", $.block_arg_list), $.block_arg_list)),

    block_arg_list: ($) => seq("[", optional($.block_items), "]"),

    block_items: ($) =>
      seq(
        $.expression,
        repeat(seq($._item_separator, $.expression)),
        optional($._item_separator),
      ),

    return_form: ($) =>
      choice(prec.right(PREC.ASSIGN, seq("@r", $.expression)), "@r"),
    // Keep @ forms explicit. A catch-all @ token would hide misspelled forms
    // from tree-sitter's error recovery.
    break_form: (_) => "@b",
    continue_form: (_) => "@c",
    try_form: ($) => seq("@t", $.expression),
    debug_form: ($) => seq("@d", $.unary_expr),
    pause_form: ($) => seq("@p", optional($.unary_expr)),
    symbolic_form: ($) => seq("@s", $.comma_expr),
    import_form: ($) => seq("@i", choice($.string, $.raw_string)),

    magic_command: (_) => token(seq("!", /[^\n]*/)),

    identifier: (_) => token(seq(IDENT_START, repeat(IDENT_CONTINUE))),

    integer: (_) => token(integerBody),
    float: (_) => token(decimalFloat),
    imaginary: (_) => token(seq(choice(decimalFloat, integerBody), "i")),

    string: ($) => $._string_content,
    unicode_scalar: ($) =>
      seq(
        "@u",
        optional(/[ \t\r]*/),
        choice($._string_content, token(/\{[0-9a-fA-F]{1,6}\}/)),
      ),
    raw_string: ($) => seq("@l", optional(/[ \t\r]*/), $._raw_string_content),
    format_string: ($) =>
      seq("@f", optional(/[ \t\r]*/), $._format_string_content),

    tag: (_) => token(seq("`", IDENT)),
    backtick: (_) => "`",
    true: (_) => choice("true", "T"),
    false: (_) => choice("false", "F"),
    inf: (_) => "inf",
    ellipsis: (_) => token("..."),

    comment: ($) => choice($.line_comment, $.block_comment),
    line_comment: (_) => token(seq("//", /[^\n]*/)),
    newline: (_) => token(/\r?\n/),
    shebang: (_) => token(seq("#!", /[^\n]*/)),
  },
});

function binary($, leftOperand, rightOperand, precedence, operator) {
  return choice(
    prec.left(
      precedence,
      seq(
        leftOperand,
        repeat1(seq(field("operator", operator), continuation($, rightOperand))),
      ),
    ),
    leftOperand,
  );
}

function continuation($, operand) {
  return seq(optional(repeat1($.newline)), operand);
}
