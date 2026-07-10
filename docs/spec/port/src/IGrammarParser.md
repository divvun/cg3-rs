# src/IGrammarParser.hpp

> [spec:cg3:def:i-grammar-parser.cg3.i-grammar-parser]
> class IGrammarParser {
>   std::ostream* ux_stderr = nullptr;
>   URegularExpression* nrules = nullptr;
>   URegularExpression* nrules_inv = nullptr;
>   Grammar* result = nullptr;
>   uint32_t verbosity = 0;
> }

> [spec:cg3:def:i-grammar-parser.cg3.i-grammar-parser.i-grammar-parser-fn]
> virtual ~IGrammarParser()

> [spec:cg3:sem:i-grammar-parser.cg3.i-grammar-parser.i-grammar-parser-fn]
> Virtual destructor of the abstract base parser. Frees the two optional ICU
> rule-name filter regexes: if `nrules` is non-null call `uregex_close(nrules)`,
> and if `nrules_inv` is non-null call `uregex_close(nrules_inv)`. These
> correspond to the `--nrules` / `--nrules-inv` command-line filters that a
> caller compiles and assigns to the parser; they are the only owned resources
> the base class releases. (For reference, the non-specced constructor
> `IGrammarParser(Grammar& res, std::ostream& ux_err)` initialises
> `ux_stderr = &ux_err` and `result = &res`; `nrules`/`nrules_inv` default to
> null and `verbosity` defaults to 0.) No other members are owned or freed.

> [spec:cg3:def:i-grammar-parser.cg3.i-grammar-parser.parse-grammar-fn]
> virtual int parse_grammar(const char* buffer, size_t length) = 0

> [spec:cg3:sem:i-grammar-parser.cg3.i-grammar-parser.parse-grammar-fn]
> Pure virtual (`= 0`), no body — part of the abstract IGrammarParser contract.
> A concrete subclass (TextualParser or BinaryGrammar) must implement it to
> parse a grammar held in an in-memory byte buffer `buffer` of `length` bytes
> into the parser's `result` Grammar, returning an int status where 0 means
> success and a fatal error terminates via CG3Quit. This is one of four public
> overloads declared alongside it — `(const UChar*, size_t)`,
> `(const std::string&)`, `(const char* filename)` — plus a protected
> `(UString&)`; only this `(const char*, size_t)` form is specced here. Model
> as a trait/interface method in the port.

> [spec:cg3:def:i-grammar-parser.cg3.i-grammar-parser.set-compatible-fn]
> virtual void setCompatible(bool compat) = 0

> [spec:cg3:sem:i-grammar-parser.cg3.i-grammar-parser.set-compatible-fn]
> Pure virtual (`= 0`), no body. Contract: subclasses implement it to enable or
> disable "compatible" parsing mode according to `compat`. (BinaryGrammar's
> override ignores the argument entirely — a no-op; a textual parser may use it
> to relax syntax.) Model as an interface/trait method.

> [spec:cg3:def:i-grammar-parser.cg3.i-grammar-parser.set-verbosity-fn]
> virtual void setVerbosity(uint32_t level) = 0

> [spec:cg3:sem:i-grammar-parser.cg3.i-grammar-parser.set-verbosity-fn]
> Pure virtual (`= 0`), no body. Contract: subclasses implement it to store the
> diagnostic verbosity `level` (higher = more warnings). BinaryGrammar's
> override assigns it to the inherited `verbosity` member, which gates optional
> warning output. Model as an interface/trait method.

