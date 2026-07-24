use super::*;

fn stop_card_debug_info() -> (DebugInfo, CodeLoc) {
    let mut debug_info = DebugInfo::default();
    let file_id = debug_info.new_file("demo.wq", "first\n  total:price*qty\nlast\n");
    let chunk = debug_info.new_chunk("calc", file_id, 5);
    assert!(debug_info.set_statement_span(
        CodeLoc { chunk, pc: 2 },
        Span {
            file_id,
            start: 6,
            end: 23,
        },
    ));
    assert!(debug_info.set_exact_span(
        CodeLoc { chunk, pc: 2 },
        Span {
            file_id,
            start: 14,
            end: 23,
        },
    ));
    (debug_info, CodeLoc { chunk, pc: 2 })
}

#[test]
fn command_styles_use_explicit_style_renderer() {
    assert_eq!(
        styled_command("continue", ColorMode::Always),
        "\x1b[32mcontinue\x1b[0m"
    );
}

#[test]
fn command_help_rows_are_indented() {
    let commands = crate::wqdb::command::COMMANDS;
    let usage_width = commands
        .iter()
        .map(|spec| command_usage_plain(spec).len())
        .max()
        .expect("wqdb commands");
    let row = help_row(&commands[0], usage_width, ColorMode::Never);

    assert!(row.starts_with("  "));
}

#[test]
fn prompt_keeps_the_active_granularity_visible() {
    assert_eq!(
        prompt(StepGranularity::Expr, 3, ColorMode::Never),
        "wqdb[expr:3] "
    );
}

#[test]
fn unavailable_stop_cards_name_the_active_mode() {
    assert_eq!(
        unavailable_stop_card(StepGranularity::Line, ColorMode::Never),
        "LINE  current location unavailable"
    );
    assert_eq!(
        unavailable_stop_card(StepGranularity::Expr, ColorMode::Never),
        "EXPR  current location unavailable"
    );
    assert_eq!(
        unavailable_stop_card(StepGranularity::Inst, ColorMode::Never),
        "INST  current location unavailable"
    );
}

#[test]
fn stale_locations_render_without_panicking() {
    let debug_info = DebugInfo::default();
    let location = CodeLoc {
        chunk: wqpl::wqdb::ChunkId(u32::MAX),
        pc: 7,
    };

    assert_eq!(
        format_loc_hint(&debug_info, location, Some("calc")),
        "pc 7 in calc (location unavailable)"
    );
    assert_eq!(
        resolved_stop_span(&debug_info, location),
        (Span::NONE, false)
    );
}

#[test]
fn line_stop_card_is_source_first_and_preserves_indentation() {
    let (debug_info, location) = stop_card_debug_info();

    let rendered = format_line_stop_card(&debug_info, location, "calc", 1, ColorMode::Never);

    assert_eq!(
        rendered,
        "LINE  demo.wq:2 in calc\n   1    first\n   2 ->   total:price*qty\n   3    last"
    );
}

#[test]
fn expression_stop_card_focuses_the_exact_span() {
    let (debug_info, location) = stop_card_debug_info();

    let rendered = format_expr_stop_card(
        &debug_info,
        location,
        "calc",
        Some("BinaryOp(Multiply)"),
        ColorMode::Never,
    );

    assert_eq!(
        rendered,
        "EXPR  demo.wq:2:9 in calc\n  2 ->   total:price*qty\n               ~~~~~~~~~\npc 2  BinaryOp(Multiply)"
    );
}

#[test]
fn expression_stop_card_preserves_pretty_instruction_color() {
    let (debug_info, location) = stop_card_debug_info();
    let instruction = "\x1b[35mBinaryOp\x1b[0m(Multiply)";

    let rendered = format_expr_stop_card(
        &debug_info,
        location,
        "calc",
        Some(instruction),
        ColorMode::Always,
    );

    assert!(
        rendered.ends_with(&format!("\x1b[90mpc 2  \x1b[0m{instruction}")),
        "card was: {rendered:?}"
    );
}

#[test]
fn instruction_stop_card_leads_with_disassembly_then_source() {
    let (debug_info, location) = stop_card_debug_info();
    let instructions = vec![
        (0, "LoadLocal(0)".to_string()),
        (1, "LoadLocal(1)".to_string()),
        (2, "BinaryOp(Multiply)".to_string()),
        (3, "StoreLocal(2)".to_string()),
    ];

    let rendered = format_inst_stop_card(
        &debug_info,
        location,
        "calc",
        5,
        &instructions,
        ColorMode::Never,
    );

    assert_eq!(
        rendered,
        "INST  calc  pc 2/4\n   0    LoadLocal(0)\n   1    LoadLocal(1)\n   2 -> BinaryOp(Multiply)\n   3    StoreLocal(2)\n\nSOURCE  demo.wq:2:9\n  2 ->   total:price*qty\n               ~~~~~~~~~"
    );
}

#[test]
fn instruction_stop_card_colors_prefix_without_overriding_opcode() {
    let (debug_info, location) = stop_card_debug_info();
    let instruction = "\x1b[35mBinaryOp\x1b[0m(Multiply)";

    let rendered = format_inst_stop_card(
        &debug_info,
        location,
        "calc",
        5,
        &[(2, instruction.to_string())],
        ColorMode::Always,
    );

    assert!(
        rendered.contains(&format!("\x1b[1;32m   2 -> \x1b[0m{instruction}")),
        "card was: {rendered:?}"
    );
}

#[test]
fn compact_instruction_preserves_complete_ansi_sequences() {
    let instruction = format!("\x1b[31mLoadConst\x1b[0m({})", "x".repeat(140));

    let compact = compact_instruction(&instruction);

    assert!(compact.starts_with("\x1b[31mLoadConst\x1b[0m("));
    assert!(compact.ends_with("…\x1b[0m"));
    assert_eq!(ansi_visible_width(&compact), 120);
}

#[test]
fn compact_instructions_count_terminal_columns() {
    let instruction = format!("\x1b[31mLoadConst\x1b[0m({})", "界".repeat(80));

    let compact = compact_instruction(&instruction);

    assert!(compact.ends_with("…\x1b[0m"));
    assert!(ansi_visible_width(&compact) <= 120);
}

#[test]
fn expression_stop_card_clamps_a_multiline_span() {
    let mut debug_info = DebugInfo::default();
    let file_id = debug_info.new_file("demo.wq", "x:(1;\n  2)\n");
    let chunk = debug_info.new_chunk("calc", file_id, 1);
    assert!(debug_info.set_exact_span(
        CodeLoc { chunk, pc: 0 },
        Span {
            file_id,
            start: 2,
            end: 10,
        },
    ));

    let rendered = format_expr_stop_card(
        &debug_info,
        CodeLoc { chunk, pc: 0 },
        "calc",
        Some("MakeList(2)"),
        ColorMode::Never,
    );

    assert_eq!(
        rendered,
        "EXPR  demo.wq:1:3 in calc\n  1 -> x:(1;\n         ~~~\npc 0  MakeList(2)"
    );
}

#[test]
fn stop_cards_keep_instruction_context_when_source_is_unavailable() {
    let mut debug_info = DebugInfo::default();
    let file_id = debug_info.new_file("demo.wq", "");
    let chunk = debug_info.new_chunk("calc", file_id, 1);
    let location = CodeLoc { chunk, pc: 0 };

    let expression = format_expr_stop_card(
        &debug_info,
        location,
        "calc",
        Some("Return"),
        ColorMode::Never,
    );
    let instruction = format_inst_stop_card(
        &debug_info,
        location,
        "calc",
        1,
        &[(0, "Return".to_string())],
        ColorMode::Never,
    );

    assert_eq!(
        expression,
        "EXPR  pc 0 in calc\n  source unavailable\npc 0  Return"
    );
    assert!(
        instruction.ends_with("\n\nSOURCE  unavailable"),
        "card was: {instruction}"
    );
}

#[test]
fn expression_stop_card_reports_display_columns() {
    let mut debug_info = DebugInfo::default();
    let file_id = debug_info.new_file("demo.wq", "α:1\n");
    let chunk = debug_info.new_chunk("calc", file_id, 1);
    assert!(debug_info.set_exact_span(
        CodeLoc { chunk, pc: 0 },
        Span {
            file_id,
            start: 3,
            end: 4,
        },
    ));

    let rendered = format_expr_stop_card(
        &debug_info,
        CodeLoc { chunk, pc: 0 },
        "calc",
        None,
        ColorMode::Never,
    );

    assert!(
        rendered.starts_with("EXPR  demo.wq:1:3 in calc"),
        "card was: {rendered}"
    );
}

#[test]
fn line_stop_card_omits_a_phantom_line_after_final_newline() {
    let mut debug_info = DebugInfo::default();
    let file_id = debug_info.new_file("demo.wq", "a:1\nb:2\n");
    let chunk = debug_info.new_chunk("calc", file_id, 1);
    assert!(debug_info.set_statement_span(
        CodeLoc { chunk, pc: 0 },
        Span {
            file_id,
            start: 4,
            end: 7,
        },
    ));

    let rendered = format_line_stop_card(
        &debug_info,
        CodeLoc { chunk, pc: 0 },
        "calc",
        2,
        ColorMode::Never,
    );

    assert!(!rendered.contains("\n   3"), "card was: {rendered}");
}
