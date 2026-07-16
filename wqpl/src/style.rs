use std::env;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorMode {
    Auto,
    Always,
    Never,
}

impl ColorMode {
    /// Resolve automatic color selection for a specific output destination.
    ///
    /// Hosts that redirect output should pass that destination's terminal
    /// capability instead of inheriting the process stdout capability.
    pub fn resolve(self, output_is_terminal: bool) -> Self {
        match self {
            Self::Auto => {
                if color_env_override().unwrap_or_else(|| {
                    clicolor_enabled(normalize_env_var("CLICOLOR"), output_is_terminal)
                }) {
                    Self::Always
                } else {
                    Self::Never
                }
            }
            mode => mode,
        }
    }

    pub fn should_colorize(self) -> bool {
        self.resolve(stdout_is_terminal()) == Self::Always
    }
}

fn color_env_override() -> Option<bool> {
    resolve_color_env_override(
        normalize_env_var("NO_COLOR"),
        normalize_env_var("CLICOLOR_FORCE"),
    )
}

fn normalize_env_var(key: &str) -> Option<bool> {
    env::var(key).ok().map(|value| value != "0")
}

fn resolve_color_env_override(
    no_color: Option<bool>,
    clicolor_force: Option<bool>,
) -> Option<bool> {
    if clicolor_force == Some(true) {
        Some(true)
    } else if no_color.is_some() {
        Some(false)
    } else {
        None
    }
}

fn clicolor_enabled(clicolor: Option<bool>, stdout_is_terminal: bool) -> bool {
    clicolor.unwrap_or(true) && stdout_is_terminal
}

#[cfg(not(target_arch = "wasm32"))]
fn stdout_is_terminal() -> bool {
    use std::io::IsTerminal as _;

    std::io::stdout().is_terminal()
}

#[cfg(target_arch = "wasm32")]
fn stdout_is_terminal() -> bool {
    false
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnsiColor {
    Black,
    Red,
    Green,
    Yellow,
    Cyan,
    Blue,
    Magenta,
    White,
    Purple,
    BrightBlack,
    BrightBlue,
    BrightGreen,
    BrightRed,
    BrightCyan,
    BrightMagenta,
    BrightYellow,
    BrightWhite,
}

impl AnsiColor {
    const fn fg_code(self) -> &'static str {
        match self {
            Self::Black => "30",
            Self::Red => "31",
            Self::Green => "32",
            Self::Yellow => "33",
            Self::Cyan => "36",
            Self::Blue => "34",
            Self::Magenta => "35",
            Self::White => "37",
            Self::Purple => "35",
            Self::BrightBlack => "90",
            Self::BrightBlue => "94",
            Self::BrightGreen => "92",
            Self::BrightRed => "91",
            Self::BrightCyan => "96",
            Self::BrightMagenta => "95",
            Self::BrightYellow => "93",
            Self::BrightWhite => "97",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TextStyle {
    fg: Option<AnsiColor>,
    bold: bool,
    dimmed: bool,
    italic: bool,
    underline: bool,
}

impl TextStyle {
    pub const fn new() -> Self {
        Self {
            fg: None,
            bold: false,
            dimmed: false,
            italic: false,
            underline: false,
        }
    }

    pub const fn fg(mut self, color: AnsiColor) -> Self {
        self.fg = Some(color);
        self
    }

    pub const fn bold(mut self) -> Self {
        self.bold = true;
        self
    }

    pub const fn dimmed(mut self) -> Self {
        self.dimmed = true;
        self
    }

    pub const fn italic(mut self) -> Self {
        self.italic = true;
        self
    }

    pub const fn underline(mut self) -> Self {
        self.underline = true;
        self
    }
}

pub fn paint(text: &str, style: TextStyle, color_mode: ColorMode) -> String {
    if !color_mode.should_colorize() {
        return text.to_string();
    }

    let mut codes = Vec::new();
    if style.bold {
        codes.push("1");
    }
    if style.dimmed {
        codes.push("2");
    }
    if style.italic {
        codes.push("3");
    }
    if style.underline {
        codes.push("4");
    }
    if let Some(color) = style.fg {
        codes.push(color.fg_code());
    }

    if codes.is_empty() {
        return text.to_string();
    }

    format!("\x1b[{}m{text}\x1b[0m", codes.join(";"))
}

pub fn plain(text: &str) -> String {
    text.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_env_override_follows_clicolor_precedence() {
        assert_eq!(resolve_color_env_override(None, None), None);
        assert_eq!(resolve_color_env_override(None, Some(false)), None);
        assert_eq!(resolve_color_env_override(Some(true), None), Some(false));
        assert_eq!(resolve_color_env_override(Some(false), None), Some(false));
        assert_eq!(
            resolve_color_env_override(Some(true), Some(false)),
            Some(false)
        );
        assert_eq!(
            resolve_color_env_override(Some(true), Some(true)),
            Some(true)
        );
    }

    #[test]
    fn clicolor_requires_enabled_value_and_terminal_stdout() {
        assert!(clicolor_enabled(None, true));
        assert!(clicolor_enabled(Some(true), true));
        assert!(!clicolor_enabled(Some(false), true));
        assert!(!clicolor_enabled(None, false));
    }

    #[test]
    fn explicit_color_modes_ignore_output_capability() {
        assert_eq!(ColorMode::Always.resolve(false), ColorMode::Always);
        assert_eq!(ColorMode::Never.resolve(true), ColorMode::Never);
    }
}
