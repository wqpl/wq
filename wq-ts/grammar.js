const PREC = {
  ASSIGN: 1,
  PIPE: 2,
  COMMA: 3,
  BOOL_OR: 4,
  BOOL_AND: 5,
  COMPARE: 6,
  BIT_OR: 7,
  BIT_XOR: 8,
  BIT_AND: 9,
  SHIFT: 10,
  ADD: 11,
  MULTIPLY: 12,
  RANGE: 13,
  UNARY: 14,
  POWER: 15,
  POSTFIX: 16,
};

const IDENT_START = /[\p{ID_Start}_]/u;
const IDENT_CONTINUE = /[\p{ID_Continue}_?]/u;
const IDENT = /[\p{ID_Start}_][\p{ID_Continue}_?]*/u;
const DEC = /[0-9](?:_?[0-9])*/;
const BIN = /[01](?:_?[01])*/;
const OCT = /[0-7](?:_?[0-7])*/;
const HEX = /[0-9a-fA-F](?:_?[0-9a-fA-F])*/;

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

module.exports = grammar({
  name: "wq",

  extras: ($) => [/[ \t\r]/, $.comment],

  externals: ($) => [
    $.block_comment,
    $._string_content,
    $._raw_string_content,
    $._format_string_content,
  ],

  word: ($) => $.identifier,

  conflicts: ($) => [
    [$.dict_literal, $.paren_list],
    [$.literal, $.dict_pair],
    [$.function_literal, $.block],
    [$.operator_identifier, $.unary_expr],
    [$.comma_expr, $.operator_identifier],
    [$.pause_form],
    [$.w_loop, $.postfix_expr],
    [$.n_loop, $.postfix_expr],
    [$.block_form, $.postfix_expr],
  ],

  rules: {
    source_file: ($) =>
      seq(optional($.shebang), repeat(choice($.statement, $.separator))),

    statement: ($) => choice($.magic_command, $.expression),

    separator: ($) => choice(";", $.newline),

    block: ($) => repeat1(choice($.statement, $.separator)),

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
        "&|:",
        "\\|:",
        "&:",
        "\\:",
        "<<:",
        ">>:",
        "^\\:",
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

    comma_expr: ($) =>
      choice(
        prec.left(
          PREC.COMMA,
          seq($.bool_or_expr, repeat1(seq(",", $.bool_or_expr))),
        ),
        prec(PREC.COMMA, repeat1(seq(",", $.bool_or_expr))),
        $.bool_or_expr,
      ),

    bool_or_expr: ($) => binary($, $.bool_and_expr, PREC.BOOL_OR, "\\|"),

    bool_and_expr: ($) => binary($, $.comparison_expr, PREC.BOOL_AND, "&|"),

    comparison_expr: ($) =>
      choice(
        prec.left(
          PREC.COMPARE,
          seq(
            $.bit_or_expr,
            repeat1(seq(field("operator", $.comparison_operator), $.bit_or_expr)),
          ),
        ),
        $.bit_or_expr,
      ),

    comparison_operator: (_) => choice("=.", "=", "~.", "~", "<=", "<", ">=", ">"),

    bit_or_expr: ($) => binary($, $.bit_xor_expr, PREC.BIT_OR, "\\"),

    bit_xor_expr: ($) => binary($, $.bit_and_expr, PREC.BIT_XOR, "^\\"),

    bit_and_expr: ($) => binary($, $.shift_expr, PREC.BIT_AND, "&"),

    shift_expr: ($) => binary($, $.additive_expr, PREC.SHIFT, choice("<<", ">>")),

    additive_expr: ($) =>
      binary($, $.multiplicative_expr, PREC.ADD, choice("+", "-")),

    multiplicative_expr: ($) =>
      binary(
        $,
        $.range_expr,
        PREC.MULTIPLY,
        choice("**", "/%", "/.", "*", "/", "%"),
      ),

    range_expr: ($) =>
      choice(
        prec.right(
          PREC.RANGE,
          seq(
            $.unary_expr,
            field("operator", choice("..=", "..")),
            $.unary_expr,
            optional(seq(field("step_operator", ".."), $.unary_expr)),
          ),
        ),
        $.unary_expr,
      ),

    unary_expr: ($) =>
      choice(
        prec(PREC.UNARY, seq(repeat1(choice("-", "#", "~")), $.power_expr)),
        $.power_expr,
      ),

    power_expr: ($) =>
      choice(
        prec.right(
          PREC.POWER,
          seq($.postfix_expr, field("operator", choice("^.", "^")), $.unary_expr),
        ),
        $.postfix_expr,
      ),

    postfix_expr: ($) =>
      choice(
        $.primary,
        prec.left(PREC.POSTFIX, seq($.postfix_expr, $.suffix)),
      ),

    suffix: ($) =>
      choice($.call_suffix, $.mutating_index_suffix, $.juxtaposition_suffix),

    call_suffix: ($) =>
      prec(PREC.POSTFIX, seq(optional($.depth_modifier), $.arg_list)),

    mutating_index_suffix: ($) =>
      prec(PREC.POSTFIX, seq("[", "!", optional($.argument_items), "]")),

    juxtaposition_suffix: ($) =>
      prec(PREC.POSTFIX, seq(optional($.depth_modifier), $.juxtaposition_arg)),

    juxtaposition_arg: ($) => $.range_expr,

    depth_modifier: (_) => token(seq("@", /[0-9](?:_?[0-9])*/)),

    arg_list: ($) => seq("[", optional($.argument_items), "]"),

    argument_items: ($) =>
      choice(
        $._item_separator,
        seq(
          $.expression,
          repeat(seq($._item_separator, $.expression)),
          optional($._item_separator),
        ),
      ),

    _item_separator: ($) =>
      prec.right(choice(
        seq(optional(repeat1($.newline)), ";", optional(repeat1($.newline))),
        repeat1($.newline),
      )),

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
        $.w_loop,
        $.n_loop,
        $.block_form,
        $.return_form,
        $.break_form,
        $.continue_form,
        $.try_form,
        $.assert_form,
        $.debug_form,
        $.pause_form,
        $.symbolic_form,
      ),

    literal: ($) =>
      choice(
        $.imaginary,
        $.float,
        $.integer,
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
        "<<",
        ">>",
        ",",
        "#",
        "&|",
        "\\|",
        "&",
        "\\",
        "^\\",
      ),

    function_literal: ($) =>
      seq(optional("'"), "{", optional($.param_list), optional($.block), "}"),

    param_list: ($) =>
      seq(
        "[",
        optional(separated1($.param, $._param_separator)),
        optional($._param_separator),
        "]",
      ),

    _param_separator: ($) =>
      prec.right(choice(
        seq(optional(repeat1($.newline)), ";", optional(repeat1($.newline))),
        repeat1($.newline),
      )),

    param: ($) => choice($.identifier, seq($.tag, optional(seq(":", $.pipe_expr)))),

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
    block_form: ($) => prec.dynamic(1, seq("B", $.arg_list)),

    return_form: ($) => choice(prec.right(PREC.ASSIGN, seq("@r", $.expression)), "@r"),
    break_form: (_) => "@b",
    continue_form: (_) => "@c",
    try_form: ($) => seq("@t", $.expression),
    assert_form: ($) => seq("@a", $.expression),
    debug_form: ($) => seq("@d", $.unary_expr),
    pause_form: ($) => seq("@p", optional($.unary_expr)),
    symbolic_form: ($) => seq("@s", $.expression),

    magic_command: (_) => token(seq("!", /[^\n]*/)),

    identifier: (_) => token(seq(IDENT_START, repeat(IDENT_CONTINUE))),

    integer: (_) => token(integerBody),
    float: (_) => token(decimalFloat),
    imaginary: (_) => token(seq(choice(decimalFloat, integerBody), "i")),

    string: ($) => $._string_content,
    raw_string: ($) => seq("@l", optional(/[ \t\r]*/), $._raw_string_content),
    format_string: ($) => seq("@f", optional(/[ \t\r]*/), $._format_string_content),

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

function binary($, operand, precedence, operator) {
  return choice(
    prec.left(
      precedence,
      seq(operand, repeat1(seq(field("operator", operator), operand))),
    ),
    operand,
  );
}
