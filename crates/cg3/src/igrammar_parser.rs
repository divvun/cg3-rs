//! Port of `src/IGrammarParser.hpp` — the contract both grammar parsers keep.
//!
//! A parser owns the grammar it builds, as the C++ `result` member binds one
//! at construction: [`TextualParser`](crate::textual_parser::TextualParser)
//! and [`BinaryGrammar`](crate::binary_grammar::BinaryGrammar) each take theirs
//! in `new` and hand it back as a field once parsing is done. The C++ base
//! class's data members (`nrules`, `nrules_inv`, `verbosity`) live on the two
//! implementors, since a trait has no fields.
//!
//! The C++ declares `parse_grammar` over four input shapes. One is modelled
//! here: bytes. A file path is the implementor's business (the binary reader
//! records it for the companion source file; the textual one names its
//! diagnostics with it), and the UTF-16 forms describe nothing a UTF-8 port
//! has.

use crate::grammar::Grammar;

// [spec:cg3:def:i-grammar-parser.cg3.i-grammar-parser]
// [spec:cg3:def:i-grammar-parser.cg3.i-grammar-parser.i-grammar-parser-fn]
// [spec:cg3:sem:i-grammar-parser.cg3.i-grammar-parser.i-grammar-parser-fn]
// The `i-grammar-parser-fn` id names the C++ `virtual ~IGrammarParser()`,
// which closes the two `--nrules` filter regexes. Those are owned `Option`
// fields on each implementor here, so ordinary drop glue is the destructor.
/// C++ `class IGrammarParser` — what a grammar parser offers regardless of the
/// format it reads.
pub trait IGrammarParser {
    // [spec:cg3:def:i-grammar-parser.cg3.i-grammar-parser.parse-grammar-fn]
    // [spec:cg3:sem:i-grammar-parser.cg3.i-grammar-parser.parse-grammar-fn]
    /// C++ pure-virtual `int parse_grammar(const char* buffer, size_t length)`.
    /// Parses `input` into this parser's own grammar. The C++ nonzero return (a
    /// count of recoverable parse errors) becomes
    /// [`GrammarError::Parse`](crate::error::GrammarError::Parse), and every
    /// other load failure its own `GrammarError` variant.
    fn parse_grammar(&mut self, input: &[u8]) -> Result<(), crate::error::Cg3Error>;

    // [spec:cg3:def:i-grammar-parser.cg3.i-grammar-parser.set-compatible-fn]
    // [spec:cg3:sem:i-grammar-parser.cg3.i-grammar-parser.set-compatible-fn]
    /// C++ pure-virtual `void setCompatible(bool compat)`: vislcg
    /// compatibility mode, which relaxes the textual syntax.
    fn set_compatible(&mut self, compat: bool);

    // [spec:cg3:def:i-grammar-parser.cg3.i-grammar-parser.set-verbosity-fn]
    // [spec:cg3:sem:i-grammar-parser.cg3.i-grammar-parser.set-verbosity-fn]
    /// C++ pure-virtual `void setVerbosity(uint32_t level)`: higher levels
    /// enable more optional warnings.
    fn set_verbosity(&mut self, level: u32);

    /// The grammar this parser builds (C++ `result`).
    fn get_grammar(&self) -> &Grammar;
}
