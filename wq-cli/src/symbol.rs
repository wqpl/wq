use std::path::Path;

use wqpl::frontend::Frontend;
use wqpl::style::{AnsiColor, ColorMode, TextStyle, paint};
use wqpl::symbol::UseKind;

pub fn run_symbols<P: AsRef<Path>>(path: P, name: &str) {
    let path = path.as_ref();
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(err) => {
            eprintln!("Cannot read {}: {err}", path.display());
            std::process::exit(1);
        }
    };

    let frontend = Frontend::default();
    let index = match frontend.analyze_symbols(&content) {
        Ok(idx) => idx,
        Err(err) => {
            eprintln!("Parse error: {err}");
            std::process::exit(1);
        }
    };

    // Try to find a def with the given name
    let mut found = false;
    for (def_idx, def) in index.defs.iter().enumerate() {
        if def.name != name {
            continue;
        }
        found = true;
        let result = if let Some((start, _)) = def.span {
            index.query_at(start)
        } else {
            index.query_def(def_idx)
        };
        if let Some(res) = result {
            print_symbol_result(&content, &res);
            println!();
        }
    }

    if !found {
        eprintln!(
            "No user-defined symbol named '{}' found in {}",
            name,
            path.display()
        );
        std::process::exit(1);
    }
}

fn print_symbol_result(src: &str, result: &wqpl::symbol::SymbolQueryResult) {
    let ref_capture_count = result
        .uses
        .iter()
        .filter(|loc| loc.kind.is_ref_capture())
        .count();
    let ref_capture_note = if ref_capture_count == 0 {
        String::new()
    } else {
        format!(
            "  {}",
            symbol_emphasis(&format!("ref captures: {ref_capture_count}"))
        )
    };
    match result.def_span {
        Some((start, _)) => {
            let (line, col) = line_col(src, start);
            println!("{} @ {}:{}{}", result.name, line, col, ref_capture_note);
        }
        None => {
            println!("{} @ <synthetic>{}", result.name, ref_capture_note);
        }
    }
    for loc in &result.uses {
        let (l, c) = line_col(src, loc.span.0);
        let kind_str = use_kind_label(loc.kind);
        let snippet = snippet_at(src, loc.span);
        println!("  {} {}:{}  {}", kind_str, l, c, snippet);
    }
}

fn use_kind_label(kind: UseKind) -> String {
    use_kind_label_with_color_mode(kind, ColorMode::Auto)
}

fn use_kind_label_with_color_mode(kind: UseKind, color_mode: ColorMode) -> String {
    match kind {
        UseKind::Read => "read       ".to_string(),
        UseKind::Write => "write      ".to_string(),
        UseKind::OuterRead => symbol_emphasis_with_color_mode("outer-ref-r", color_mode),
        UseKind::OuterWrite => symbol_emphasis_with_color_mode("outer-ref-w", color_mode),
        UseKind::RefCaptureRead => symbol_emphasis_with_color_mode("ref-read   ", color_mode),
        UseKind::RefCaptureWrite => symbol_emphasis_with_color_mode("ref-write  ", color_mode),
    }
}

fn symbol_emphasis(text: &str) -> String {
    symbol_emphasis_with_color_mode(text, ColorMode::Auto)
}

fn symbol_emphasis_with_color_mode(text: &str, color_mode: ColorMode) -> String {
    paint(
        text,
        TextStyle::new().fg(AnsiColor::Magenta).bold(),
        color_mode,
    )
}

fn line_col(src: &str, byte_offset: usize) -> (usize, usize) {
    let mut line = 1;
    let mut col = 1;
    for (i, ch) in src.char_indices() {
        if i >= byte_offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, col)
}

fn snippet_at(src: &str, span: (usize, usize)) -> String {
    let start = span.0;
    let end = span.1;
    if start >= src.len() || end > src.len() || start >= end {
        return String::new();
    }
    let slice = &src[start..end];
    // If it contains a newline, truncate
    if let Some(pos) = slice.find('\n') {
        slice[..pos].to_string()
    } else {
        slice.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ref_capture_labels_use_explicit_style_renderer() {
        assert_eq!(
            use_kind_label_with_color_mode(UseKind::RefCaptureRead, ColorMode::Always),
            "\x1b[1;35mref-read   \x1b[0m"
        );
        assert_eq!(
            use_kind_label_with_color_mode(UseKind::Read, ColorMode::Always),
            "read       "
        );
    }
}
