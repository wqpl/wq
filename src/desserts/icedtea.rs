// icedtea: collection of miscellaneous small helpers

pub fn create_boxed_text(text: &str, padding: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let max_len = lines.iter().map(|line| line.len()).max().unwrap_or(0);
    let total_width = max_len + padding * 2;

    let top_border = format!("+{}+", "-".repeat(total_width));
    let bottom_border = top_border.clone();

    let mut result = String::new();
    result.push_str(&top_border);
    result.push('\n');

    for line in lines {
        let spaces = max_len - line.len();
        result.push_str(&format!(
            "|{}{}{}|\n",
            " ".repeat(padding),
            line,
            " ".repeat(padding + spaces)
        ));
    }

    result.push_str(&bottom_border);
    result
}
