#[derive(Debug, Clone, PartialEq)]
pub enum WqError {
    VmError(String),

    EofError(String),
    ValueError(String),
    DomainError(String),
    LengthError(String),
    SyntaxError(String),
    ArityError(String),
    IndexError(String),
    AssertionError(String),
    IoError(String),
    EncodeError(String),
    ExecError(String),

    ArithmeticOverflowError(String),
    ZeroDivisionError(String),

    UnknownError(String, i32),
}

macro_rules! define_wq_errors {
    ( $( $code_ident:ident => ($variant:ident, $num:expr, $ctor:expr) ),+ $(,)? ) => {
        // #[derive(Copy, Clone, Debug, PartialEq, Eq)]
        // #[repr(i32)]
        // enum WqErrCode { $( $code_ident = $num ),+ }

        // impl core::convert::TryFrom<i32> for WqErrCode {
        //     type Error = ();
        //     fn try_from(v: i32) -> Result<Self, ()> {
        //         match v { $( $num => Ok(WqErrCode::$code_ident), )+ _ => Err(()) }
        //     }
        // }

        impl WqError {
            /// Numeric error code (macro-provided for all known variants).
            pub fn code(&self) -> i32 {
                match self {
                    WqError::UnknownError(_, c) => *c,
                    $( WqError::$variant(..) => $num, )+
                }
            }

            /// Construct a concrete `WqError` for a known code with a custom message.
            /// Returns `None` if `code` is not one of the macro-defined codes.
            pub fn from_code_with_msg(code: i32, msg: String) -> Option<Self> {
                match code { $( $num => Some($ctor(msg)), )+ _ => None }
            }

            /// Return the error message without any label/prefix.
            pub fn message(&self) -> &str {
                match self {
                    $( WqError::$variant(msg) => msg, )+
                    WqError::UnknownError(msg, _) => msg,
                }
            }

            /// Return a String listing all error names + numbers, column-aligned
            pub fn dump_error_codes() -> String {
                // array of (name, number) produced by the macro
                let items: &'static [(&'static str, i32)] = &[
                    $( (stringify!($code_ident), $num), )+
                ];

                // find max name length for column alignment
                let max_name = items.iter().map(|(s, _)| s.len()).max().unwrap_or(0);

                // build output
                let mut out = String::new();
                for (name, num) in items {
                    // left-align name to width `max_name`, then 2 spaces and the number
                    out.push_str(&format!("{:<width$}  {}\n", name, num, width = max_name));
                }
                // trim final newline if any
                let _ = out.pop();
                out
            }
        }

        impl std::error::Error for WqError {}


    }
}

define_wq_errors! {
    Vm => (VmError, -9999, WqError::VmError),

    Eof => (EofError, -1, WqError::EofError),
    Syntax => (SyntaxError, -2, WqError::SyntaxError),
    Value => (ValueError, -3, WqError::ValueError),
    Index => (IndexError, -4, WqError::IndexError),
    Arity => (ArityError, -5, WqError::ArityError),

    Domain => (DomainError, 1, WqError::DomainError),
    Length => (LengthError, 2, WqError::LengthError),

    ArithmeticOverflow => (ArithmeticOverflowError, 100, WqError::ArithmeticOverflowError),
    ZeroDivision => (ZeroDivisionError, 101, WqError::ZeroDivisionError),

    Assertion => (AssertionError, 200, WqError::AssertionError),
    Io => (IoError, 201, WqError::IoError),
    Encode => (EncodeError, 202, WqError::EncodeError),
    Exec => (ExecError, 203, WqError::ExecError),
}

impl std::fmt::Display for WqError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use WqError::*;
        let (label, msg) = match self {
            EofError(msg) => ("EOF ERROR", msg),
            VmError(msg) => ("!!VM ERROR", msg),
            SyntaxError(msg) => ("SYNTAX ERROR", msg),
            ValueError(msg) => ("VALUE ERROR", msg),
            DomainError(msg) => ("DOMAIN ERROR", msg),
            LengthError(msg) => ("LENGTH ERROR", msg),
            ArityError(msg) => ("ARITY ERROR", msg),
            IndexError(msg) => ("INDEX ERROR", msg),
            ExecError(msg) => ("EXEC ERROR", msg),
            AssertionError(msg) => ("ASSERTION ERROR", msg),
            IoError(msg) => ("IO ERROR", msg),
            EncodeError(msg) => ("ENCODE ERROR", msg),
            ArithmeticOverflowError(msg) => ("ARITHMETIC OVERFLOW ERROR", msg),
            ZeroDivisionError(msg) => ("ZERO DIVISION ERROR", msg),
            UnknownError(msg, _) => ("(UNKNOWN ERROR)", msg),
        };
        write!(f, "{label} ({}): {msg}", self.code())
    }
}
