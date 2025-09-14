#[cfg(not(target_arch = "wasm32"))]
pub use colored::{Color, Colorize};

#[cfg(target_arch = "wasm32")]
pub type ColoredString = str;

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Copy)]
pub enum Color {
    Black,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    White,
    BrightBlack,
    BrightRed,
    BrightGreen,
    BrightYellow,
    BrightBlue,
    BrightMagenta,
    BrightCyan,
    BrightWhite,
    Purple,
}

#[cfg(target_arch = "wasm32")]
pub trait Colorize {
    fn color(&self, _c: Color) -> &ColoredString;
    fn bold(&self) -> &ColoredString;
    fn italic(&self) -> &ColoredString;
    fn underline(&self) -> &ColoredString;
    fn dimmed(&self) -> &ColoredString;

    fn black(&self) -> &ColoredString;
    fn red(&self) -> &ColoredString;
    fn green(&self) -> &ColoredString;
    fn yellow(&self) -> &ColoredString;
    fn blue(&self) -> &ColoredString;
    fn magenta(&self) -> &ColoredString;
    fn purple(&self) -> &ColoredString;
    fn cyan(&self) -> &ColoredString;
    fn white(&self) -> &ColoredString;

    fn bright_black(&self) -> &ColoredString;
    fn bright_red(&self) -> &ColoredString;
    fn bright_green(&self) -> &ColoredString;
    fn bright_yellow(&self) -> &ColoredString;
    fn bright_blue(&self) -> &ColoredString;
    fn bright_magenta(&self) -> &ColoredString;
    fn bright_cyan(&self) -> &ColoredString;
    fn bright_white(&self) -> &ColoredString;
}

#[cfg(target_arch = "wasm32")]
impl<T: AsRef<str>> Colorize for T {
    #[inline]
    fn color(&self, _c: Color) -> &ColoredString {
        self.as_ref()
    }
    #[inline]
    fn bold(&self) -> &ColoredString {
        self.as_ref()
    }
    #[inline]
    fn italic(&self) -> &ColoredString {
        self.as_ref()
    }
    #[inline]
    fn underline(&self) -> &ColoredString {
        self.as_ref()
    }
    #[inline]
    fn dimmed(&self) -> &ColoredString {
        self.as_ref()
    }

    #[inline]
    fn black(&self) -> &ColoredString {
        self.as_ref()
    }
    #[inline]
    fn red(&self) -> &ColoredString {
        self.as_ref()
    }
    #[inline]
    fn green(&self) -> &ColoredString {
        self.as_ref()
    }
    #[inline]
    fn yellow(&self) -> &ColoredString {
        self.as_ref()
    }
    #[inline]
    fn blue(&self) -> &ColoredString {
        self.as_ref()
    }
    #[inline]
    fn magenta(&self) -> &ColoredString {
        self.as_ref()
    }
    #[inline]
    fn purple(&self) -> &ColoredString {
        self.as_ref()
    }
    #[inline]
    fn cyan(&self) -> &ColoredString {
        self.as_ref()
    }
    #[inline]
    fn white(&self) -> &ColoredString {
        self.as_ref()
    }

    #[inline]
    fn bright_black(&self) -> &ColoredString {
        self.as_ref()
    }
    #[inline]
    fn bright_red(&self) -> &ColoredString {
        self.as_ref()
    }
    #[inline]
    fn bright_green(&self) -> &ColoredString {
        self.as_ref()
    }
    #[inline]
    fn bright_yellow(&self) -> &ColoredString {
        self.as_ref()
    }
    #[inline]
    fn bright_blue(&self) -> &ColoredString {
        self.as_ref()
    }
    #[inline]
    fn bright_magenta(&self) -> &ColoredString {
        self.as_ref()
    }
    #[inline]
    fn bright_cyan(&self) -> &ColoredString {
        self.as_ref()
    }
    #[inline]
    fn bright_white(&self) -> &ColoredString {
        self.as_ref()
    }
}
