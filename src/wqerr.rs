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
            W::NotBound => "binding-err",
            W::Index => "index-err",
            W::Call => "call-err",
            W::Arity => "arity-err",
            W::Domain => "domain-err",
            W::Length => "length-err",
            W::NumericOverflow => "numeric-overflow",
            W::ZeroDiv => "zero-div-err",
            W::Io => "io-err",
            W::Encode => "encoding-err",
            W::Exec => "exec-err",
            W::Raise => "raise",
        }
    }

    pub const fn to_code(&self) -> u16 {
        use WqErrType::*;
        match self {
            Vm => 1,
            Eof | Syntax => 2,
            NotBound | Index | Call | Arity | Domain | Length | NumericOverflow | ZeroDiv | Io
            | Encode | Exec | Raise => 3,
        }
    }
}

impl std::fmt::Display for WqErrType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

impl std::fmt::Display for WqErr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut output = String::new();
        use std::fmt::Write;

        write!(output, "{}: ", self.err_type.name().bold().underline())?;
        if let Some(m) = &self.msg {
            write!(output, "{m}")?;
        }
        writeln!(output)?;
        if let Some(s) = &self.src {
            writeln!(output, "@ {s}")?;
        }
        if !self.notes.is_empty() {
            for note in self.notes.iter() {
                let prefix = "● ";
                let cont = " ".repeat(prefix.chars().count());
                let mut lines = note.lines();
                if let Some(first) = lines.next() {
                    writeln!(output, "{}{}", prefix, first)?;
                }
                for line in lines {
                    writeln!(output, "{}{}", cont, line)?;
                }
            }
        }
        write!(f, "{}", output.trim_end_matches('\n'))
    }
}
