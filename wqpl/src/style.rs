use std::env;
use std::sync::{
    LazyLock,
    atomic::{AtomicU8, Ordering},
};

const COLOR_OVERRIDE_INHERIT: u8 = 0;
const COLOR_OVERRIDE_OFF: u8 = 1;
const COLOR_OVERRIDE_ON: u8 = 2;

static COLOR_OVERRIDE: AtomicU8 = AtomicU8::new(COLOR_OVERRIDE_INHERIT);
static SHOULD_COLORIZE_AUTO: LazyLock<bool> = LazyLock::new(|| {
    color_env_override()
        .unwrap_or_else(|| clicolor_enabled(normalize_env_var("CLICOLOR"), stdout_is_terminal()))
});

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorMode {
    Auto,
    Always,
    Never,
}

impl ColorMode {
    pub fn should_colorize(self) -> bool {
        match self {
            Self::Auto => color_override().unwrap_or_else(should_colorize_auto),
            Self::Always => true,
            Self::Never => false,
        }
    }
}

pub fn set_color_override(on: Option<bool>) {
    COLOR_OVERRIDE.store(encode_color_override(on), Ordering::Relaxed);
}

pub fn color_override() -> Option<bool> {
    decode_color_override(COLOR_OVERRIDE.load(Ordering::Relaxed))
}

fn should_colorize_auto() -> bool {
    *SHOULD_COLORIZE_AUTO
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

const fn encode_color_override(on: Option<bool>) -> u8 {
    match on {
        Some(true) => COLOR_OVERRIDE_ON,
        Some(false) => COLOR_OVERRIDE_OFF,
        None => COLOR_OVERRIDE_INHERIT,
    }
}

const fn decode_color_override(raw: u8) -> Option<bool> {
    match raw {
        COLOR_OVERRIDE_ON => Some(true),
        COLOR_OVERRIDE_OFF => Some(false),
        _ => None,
    }
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
    fn color_override_encoding_round_trips_without_global_mutation() {
        assert_eq!(decode_color_override(encode_color_override(None)), None);
        assert_eq!(decode_color_override(encode_color_override(Some(false))), Some(false));
        assert_eq!(decode_color_override(encode_color_override(Some(true))), Some(true));
        assert_eq!(decode_color_override(42), None);
    }

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
        assert_eq!(resolve_color_env_override(Some(true), Some(true)), Some(true));
    }

    #[test]
    fn clicolor_requires_enabled_value_and_terminal_stdout() {
        assert!(clicolor_enabled(None, true));
        assert!(clicolor_enabled(Some(true), true));
        assert!(!clicolor_enabled(Some(false), true));
        assert!(!clicolor_enabled(None, false));
    }
}
