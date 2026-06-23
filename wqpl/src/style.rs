#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorMode {
    Auto,
    Always,
    Never,
}

impl ColorMode {
    pub fn should_colorize(self) -> bool {
        match self {
            Self::Auto => colored::control::SHOULD_COLORIZE.should_colorize(),
            Self::Always => true,
            Self::Never => false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnsiColor {
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
    BrightRed,
    BrightMagenta,
    BrightYellow,
}

impl AnsiColor {
    const fn fg_code(self) -> &'static str {
        match self {
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
            Self::BrightRed => "91",
            Self::BrightMagenta => "95",
            Self::BrightYellow => "93",
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
