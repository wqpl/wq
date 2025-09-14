use crate::colored::Colorize;

#[derive(Debug, Clone, PartialEq)]
pub struct WqErr {
    pub err_type: WqErrType,
    pub src: Option<String>,
    pub msg: Option<String>,
    pub notes: Vec<String>,
}

impl WqErr {
    pub fn new(err_type: WqErrType) -> Self {
        Self {
            err_type,
            src: None,
            msg: None,
            notes: Vec::new(),
        }
    }

    pub fn src(mut self, d: impl std::fmt::Display) -> Self {
        self.src = Some(d.to_string());
        self
    }

    pub fn msg(mut self, d: impl std::fmt::Display) -> Self {
        self.msg = Some(d.to_string());
        self
    }

    pub fn attach_note(mut self, d: impl std::fmt::Display) -> Self {
        self.notes.push(d.to_string());
        self
    }

    // pub fn mut_attach_note(&mut self, s: impl Into<String>) -> &mut Self {
    //     self.notes.push(s.into());
    //     self
    // }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WqErrType {
    Vm,

    Eof,
    Syntax,
    NotBound,
    Index,
    Call,
    Arity,

    Domain,
    Length,

    NumericOverflow,
    ZeroDiv,

    Io,
    Encode,
    Exec,
    Raise,
}

impl WqErrType {
    pub const fn is_runtime(&self) -> bool {
        !self.is_compile_time()
    }

    pub const fn is_compile_time(&self) -> bool {
        use WqErrType as W;
        matches!(self, W::Eof | W::Syntax)
    }

    pub const fn name(&self) -> &'static str {
        use WqErrType as W;
        match self {
            W::Vm => "vm-error",
            W::Eof => "eof-err",
            W::Syntax => "syntax-err",
            W::NotBound => "not-bound-err",
            W::Index => "index-err",
            W::Call => "call-err",
            W::Arity => "arity-err",
            W::Domain => "domain-err",
            W::Length => "length-err",
            W::NumericOverflow => "numeric-overflow",
            W::ZeroDiv => "zero-div-err",
            W::Io => "io-err",
            W::Encode => "encode-err",
            W::Exec => "exec-err",
            W::Raise => "raise",
        }
    }

    pub const fn to_code(&self) -> u16 {
        use WqErrType::*;
        match self {
            Vm => 1,
            Eof => 2,
            Syntax => 3,
            NotBound => 4,
            Index => 5,
            Call => 6,
            Arity => 7,
            Domain => 8,
            Length => 9,
            NumericOverflow => 10,
            ZeroDiv => 11,
            Io => 12,
            Encode => 13,
            Exec => 14,
            Raise => 15,
        }
    }

    pub const fn from_code(code: u16) -> Option<Self> {
        use WqErrType::*;
        Some(match code {
            1 => Vm,
            2 => Eof,
            3 => Syntax,
            4 => NotBound,
            5 => Index,
            6 => Call,
            7 => Arity,
            8 => Domain,
            9 => Length,
            10 => NumericOverflow,
            11 => ZeroDiv,
            12 => Io,
            13 => Encode,
            14 => Exec,
            15 => Raise,
            _ => return None,
        })
    }
}

impl std::fmt::Display for WqErrType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

impl std::fmt::Display for WqErr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} ({}): ",
            self.err_type.name().bold().underline(),
            self.err_type.to_code()
        )?;
        if let Some(m) = &self.msg {
            write!(f, "{m}")?;
        }
        writeln!(f)?;
        if let Some(s) = &self.src {
            writeln!(f, "@ {s}")?;
        }
        if !self.notes.is_empty() {
            // writeln!(f, "notes:")?;
            for note in self.notes.iter() {
                // let prefix = format!("  {}. ", i + 1); // e.g. "  1. "
                let prefix = "● ";
                let cont = " ".repeat(prefix.chars().count()); // same width, spaces only

                let mut lines = note.lines();
                if let Some(first) = lines.next() {
                    writeln!(f, "{}{}", prefix, first)?;
                }
                for line in lines {
                    writeln!(f, "{}{}", cont, line)?;
                }
            }
        }
        Ok(())
    }
}
