#[path = "cli/article_fences.rs"]
mod article_fences;
#[path = "cli/dap.rs"]
mod dap;
#[path = "cli/exit_status.rs"]
mod exit_status;
#[path = "cli/help.rs"]
mod help;
#[path = "cli/print_box.rs"]
mod print_box;
#[path = "cli/script_directives.rs"]
mod script_directives;

fn strip_ansi(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            for c in chars.by_ref() {
                if c.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(ch);
        }
    }
    out
}
