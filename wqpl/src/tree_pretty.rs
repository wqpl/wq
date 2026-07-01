use crate::style::{AnsiColor, ColorMode, TextStyle, paint};

/// Flat and broken forms of a printed tree.
pub(crate) struct Pretty {
    pub(crate) flat: String,
    pub(crate) flat_len: usize,
    pub(crate) multi: String,
}

#[derive(Clone, Copy)]
pub(crate) enum HeadStyle {
    Whole,
    FirstWord,
}

/// Strip ANSI escape sequences to get the visible width of a string.
fn visible_len(s: &str) -> usize {
    let mut len = 0;
    let mut in_escape = false;
    for c in s.chars() {
        if in_escape {
            if c.is_ascii_alphabetic() {
                in_escape = false;
            }
        } else if c == '\x1b' {
            in_escape = true;
        } else {
            len += 1;
        }
    }
    len
}

fn budget(depth: usize) -> usize {
    60usize.saturating_sub(depth * 2)
}

fn pretty_style(color: AnsiColor) -> TextStyle {
    TextStyle::new().fg(color).bold()
}

fn pretty_paint(text: &str, color: AnsiColor) -> String {
    paint(text, pretty_style(color), ColorMode::Always)
}

pub(crate) fn leaf(text: &str, note: &str, color: AnsiColor) -> Pretty {
    let body = pretty_paint(text, color);
    let flat = if note.is_empty() {
        body
    } else {
        format!("{body}{note}")
    };
    let flat_len = visible_len(&flat);
    Pretty {
        flat: flat.clone(),
        flat_len,
        multi: flat,
    }
}

/// Color the first word of a head string while leaving trailing notes
/// untouched.
fn colorize_first_word(head: &str, color: AnsiColor) -> String {
    if head.starts_with('(') {
        return head.to_string();
    }

    let (label, rest) = match head.find(char::is_whitespace) {
        Some(pos) => (&head[..pos], &head[pos..]),
        None => (head, ""),
    };

    if label.contains('\x1b') {
        head.to_string()
    } else {
        format!("{}{}", pretty_paint(label, color), rest)
    }
}

fn colorize_head(head: &str, color: AnsiColor, style: HeadStyle) -> String {
    match style {
        HeadStyle::Whole => pretty_paint(head, color),
        HeadStyle::FirstWord => colorize_first_word(head, color),
    }
}

pub(crate) fn group(
    depth: usize,
    head: String,
    children: Vec<Pretty>,
    color: AnsiColor,
    head_style: HeadStyle,
    force_multi: bool,
) -> Pretty {
    let colored_head = colorize_head(&head, color, head_style);
    let open = pretty_paint("(", color);
    let close = pretty_paint(")", color);

    let mut flat_parts = vec![colored_head.clone()];
    flat_parts.extend(children.iter().map(|c| c.flat.clone()));
    let flat_body = flat_parts.join(" ");
    let flat = format!("{open}{flat_body}{close}");
    let flat_len = visible_len(&flat);

    if flat_len <= budget(depth) && !force_multi {
        Pretty {
            flat: flat.clone(),
            flat_len,
            multi: flat,
        }
    } else {
        let mut lines = vec![format!("{open}{}", colored_head)];
        for child in children {
            let child_text = if child.flat_len <= 20 {
                child.flat.clone()
            } else {
                child.multi.clone()
            };
            for line in child_text.lines() {
                lines.push(format!("  {line}"));
            }
        }
        if let Some(last) = lines.last_mut() {
            last.push_str(&close);
        } else {
            lines.push(close);
        }
        let multi = lines.join("\n");
        Pretty {
            flat,
            flat_len,
            multi,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multiline_layout_is_explicit_not_head_driven() {
        let children = vec![
            leaf("A", "", AnsiColor::White),
            leaf("B", "", AnsiColor::White),
        ];

        let doc = group(
            0,
            "LIST".to_string(),
            children,
            AnsiColor::White,
            HeadStyle::Whole,
            false,
        );

        assert!(!doc.multi.contains('\n'), "{:?}", doc.multi);
    }

    #[test]
    fn caller_can_force_multiline_layout() {
        let children = vec![
            leaf("A", "", AnsiColor::White),
            leaf("B", "", AnsiColor::White),
        ];

        let doc = group(
            0,
            "CALL [0..2]".to_string(),
            children,
            AnsiColor::White,
            HeadStyle::Whole,
            true,
        );

        assert!(doc.multi.contains('\n'), "{:?}", doc.multi);
    }
}
