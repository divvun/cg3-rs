//! Port of `src/IGrammarParser.hpp` — the abstract grammar-parser interface.
//!
//! Literal, bug-for-bug 1:1 translation (Wave 2). The C++ `class IGrammarParser`
//! is an abstract base with pure-virtual parsing hooks; here it becomes a
//! [`trait IGrammarParser`](IGrammarParser). `TextualParser` and `BinaryGrammar`
//! will `impl` it in a later pass.
//!
//! ## Class → trait mapping
//! * **Data members.** The C++ base class carries `std::ostream* ux_stderr`,
//!   `URegularExpression* nrules`, `URegularExpression* nrules_inv`,
//!   `Grammar* result`, and `uint32_t verbosity`. A Rust trait has no fields, so
//!   these live on the concrete implementor structs (e.g. `TextualParser`,
//!   `BinaryGrammar`), which reconcile them with this trait's methods.
//! * **Constructor.** The non-specced C++ ctor
//!   `IGrammarParser(Grammar& res, std::ostream& ux_err)` (sets `result = &res`,
//!   `ux_stderr = &ux_err`; `nrules`/`nrules_inv` null; `verbosity` 0) has no
//!   trait analog — traits have no constructor. Each implementor provides its own.
//! * **Virtual destructor** (`[spec:cg3:def:i-grammar-parser.cg3.i-grammar-parser.i-grammar-parser-fn]`).
//!   Deliberately **not** modelled on the trait: a trait cannot declare a
//!   destructor. The C++ dtor frees the two owned ICU filter regexes (the
//!   `--nrules` / `--nrules-inv` filters) via `uregex_close`; since those
//!   resources live on the concrete implementor, that cleanup belongs to the
//!   implementor's [`Drop`] impl.
//! * **`parse_grammar` overloads.** The C++ declares four public overloads —
//!   `(const char*, size_t)`, `(const UChar*, size_t)`, `(const std::string&)`,
//!   `(const char* filename)` — plus a protected `(UString&)`. Rust has no
//!   overloading; the trait exposes the single spec-modelled buffer form
//!   `(const char* buffer, size_t length)` as `parse_grammar(.., input: &[u8])`.
//!   The other forms (notably the filename overload) can be added by the
//!   implementors or as helpers in a later pass.

use crate::grammar::Grammar;

// [spec:cg3:def:i-grammar-parser.cg3.i-grammar-parser]
// [spec:cg3:def:i-grammar-parser.cg3.i-grammar-parser.i-grammar-parser-fn]
// [spec:cg3:sem:i-grammar-parser.cg3.i-grammar-parser.i-grammar-parser-fn]
// NOTE: the `i-grammar-parser-fn` id names the C++ `virtual ~IGrammarParser()`
// destructor (which `uregex_close`s the two owned `--nrules`/`--nrules-inv`
// filter regexes). A Rust trait has no destructor and no constructor; that
// cleanup belongs to each concrete implementor's `Drop` impl. Modelled here as a
// no-op associated default so the manifest id has a target home on the trait.
/// C++ `class IGrammarParser` — the abstract base parser contract. See the
/// module docs for how the base-class data members / ctor / dtor map onto Rust.
pub trait IGrammarParser {
    /// No-op default for the C++ `virtual ~IGrammarParser()` destructor. On the
    /// trait model there are no owned resources to release; concrete implementors
    /// free their ICU filter regexes in `Drop`.
    fn drop_parser(&mut self) {}

    // [spec:cg3:def:i-grammar-parser.cg3.i-grammar-parser.parse-grammar-fn]
    // [spec:cg3:sem:i-grammar-parser.cg3.i-grammar-parser.parse-grammar-fn]
    /// C++ pure-virtual `int parse_grammar(const char* buffer, size_t length) = 0`.
    /// Parses the grammar held in the in-memory byte buffer `input` into
    /// `grammar`, returning an `int` status where `0` means success (a fatal
    /// error terminates the process via `cg3_quit`). The C++ `(const char*,
    /// size_t)` buffer+length pair collapses to Rust `input: &[u8]`; the
    /// destination `Grammar*` (the C++ `result` member) is passed as
    /// `&mut Grammar`.
    fn parse_grammar(&mut self, grammar: &mut Grammar, input: &[u8]) -> i32;

    // [spec:cg3:def:i-grammar-parser.cg3.i-grammar-parser.set-compatible-fn]
    // [spec:cg3:sem:i-grammar-parser.cg3.i-grammar-parser.set-compatible-fn]
    /// C++ pure-virtual `void setCompatible(bool compat) = 0`. Enable/disable
    /// "compatible" parsing mode per `compat`. (`BinaryGrammar`'s override
    /// ignores it — a no-op; a textual parser may use it to relax syntax.)
    fn set_compatible(&mut self, compat: bool);

    // [spec:cg3:def:i-grammar-parser.cg3.i-grammar-parser.set-verbosity-fn]
    // [spec:cg3:sem:i-grammar-parser.cg3.i-grammar-parser.set-verbosity-fn]
    /// C++ pure-virtual `void setVerbosity(uint32_t level) = 0`. Store the
    /// diagnostic verbosity `level` (higher = more warnings). Implementors keep
    /// it in the inherited `verbosity` member, which gates optional warnings.
    fn set_verbosity(&mut self, level: u32);

    /// Accessor for the parser's built `result` grammar. No C++ spec id — this
    /// is a port addition (the C++ exposes the result via the public `result`
    /// pointer / the `Grammar&` handed to the ctor). NOTE(lead): with
    /// `parse_grammar` taking the destination `&mut Grammar` per-call, an
    /// implementor that does not retain that grammar may not be able to satisfy
    /// this; whether the trait keeps `get_grammar` (vs. relying solely on the
    /// caller-owned `Grammar`) is a reconciliation point once the concrete
    /// parsers land.
    fn get_grammar(&self) -> &Grammar;
}
