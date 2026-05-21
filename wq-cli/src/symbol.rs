use std::path::Path;

use colored::Colorize;
use wqpl::session::Session;
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

    let session = Session::new();
    let index = match session.analyze_symbols(&content) {
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
            format!("ref captures: {ref_capture_count}")
                .magenta()
                .bold()
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
        let kind_str = match loc.kind {
            UseKind::Read => "read       ".normal(),
            UseKind::Write => "write      ".normal(),
            UseKind::OuterRead => "outer-ref-r".magenta().bold(),
            UseKind::OuterWrite => "outer-ref-w".magenta().bold(),
            UseKind::RefCaptureRead => "ref-read   ".magenta().bold(),
            UseKind::RefCaptureWrite => "ref-write  ".magenta().bold(),
        };
        let snippet = snippet_at(src, loc.span);
        println!("  {} {}:{}  {}", kind_str, l, c, snippet);
    }
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
