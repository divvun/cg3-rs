//! The errors the glue raises, which Swift's `CG3Error` carries on with their
//! code.

use std::fmt;

// [spec:cg3:req:swift.errors]
/// A failed call, under the code `CG3Error` gives it in Swift, with the
/// library error's message.
#[derive(Debug, PartialEq, Eq, uniffi::Error)]
pub enum Cg3Error {
    /// An argument or option the library cannot use.
    InvalidArgument { message: String },
    /// Bytes that are not a `.cg3b` the library reads.
    InvalidGrammar { message: String },
    /// A run, conversion or split the library reports as failed.
    RunFailed { message: String },
}

impl Cg3Error {
    pub fn invalid_argument(message: impl fmt::Display) -> Cg3Error {
        Cg3Error::InvalidArgument {
            message: message.to_string(),
        }
    }

    pub fn invalid_grammar(message: impl fmt::Display) -> Cg3Error {
        Cg3Error::InvalidGrammar {
            message: message.to_string(),
        }
    }

    pub fn run_failed(message: impl fmt::Display) -> Cg3Error {
        Cg3Error::RunFailed {
            message: message.to_string(),
        }
    }
}

impl fmt::Display for Cg3Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (Cg3Error::InvalidArgument { message }
        | Cg3Error::InvalidGrammar { message }
        | Cg3Error::RunFailed { message }) = self;
        f.write_str(message)
    }
}

impl std::error::Error for Cg3Error {}
