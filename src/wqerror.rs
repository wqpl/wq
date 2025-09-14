#[derive(Debug, Clone, PartialEq)]
pub enum WqError {
    Vm(String),

    Eof(String),
    Syntax(String),
    Value(String),
    Index(String),
    Arity(String),

    Domain(String),
    Length(String),

    ArithmeticOverflow(String),
    ZeroDivision(String),

    Assert(String),
    Io(String),
    Encode(String),
    Exec(String),

    Unknown(String, i32),
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
                    WqError::Unknown(_, c) => *c,
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
                    WqError::Unknown(msg, _) => msg,
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
    Vm => (Vm, -9999, WqError::Vm),

    Eof => (Eof, -1, WqError::Eof),
    Syntax => (Syntax, -2, WqError::Syntax),
    Value => (Value, -3, WqError::Value),
    Index => (Index, -4, WqError::Index),
    Arity => (Arity, -5, WqError::Arity),

    Domain => (Domain, 1, WqError::Domain),
    Length => (Length, 2, WqError::Length),

    ArithmeticOverflow => (ArithmeticOverflow, 100, WqError::ArithmeticOverflow),
    ZeroDivision => (ZeroDivision, 101, WqError::ZeroDivision),

    Assertion => (Assert, 200, WqError::Assert),
    Io => (Io, 201, WqError::Io),
    Encode => (Encode, 202, WqError::Encode),
    Exec => (Exec, 203, WqError::Exec),
}

impl std::fmt::Display for WqError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (label, msg) = match self {
            WqError::Eof(msg) => ("EOF ERROR", msg),
            WqError::Vm(msg) => ("!!VM ERROR", msg),
            WqError::Syntax(msg) => ("SYNTAX ERROR", msg),
            WqError::Value(msg) => ("VALUE ERROR", msg),
            WqError::Domain(msg) => ("DOMAIN ERROR", msg),
            WqError::Length(msg) => ("LENGTH ERROR", msg),
            WqError::Arity(msg) => ("ARITY ERROR", msg),
            WqError::Index(msg) => ("INDEX ERROR", msg),
            WqError::Exec(msg) => ("EXEC ERROR", msg),
            WqError::Assert(msg) => ("ASSERTION ERROR", msg),
            WqError::Io(msg) => ("IO ERROR", msg),
            WqError::Encode(msg) => ("ENCODE ERROR", msg),
            WqError::ArithmeticOverflow(msg) => ("ARITHMETIC OVERFLOW ERROR", msg),
            WqError::ZeroDivision(msg) => ("ZERO DIVISION ERROR", msg),
            WqError::Unknown(msg, _) => ("(UNKNOWN ERROR)", msg),
        };
        write!(f, "{label} ({}): {msg}", self.code())
    }
}
