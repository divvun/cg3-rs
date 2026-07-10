# src/MathParser.hpp

> [spec:cg3:def:math-parser.cg3.math-parser]
> class MathParser {
>   enum type_t:uint8_t { DELIMITER = 1, VARIABLE, NUMBER, FUNCTION };
>   constexpr static size_t NUMVARS = 26;
>   UStringView exp_ptr;
>   UStringView token;
>   char tok_type = 0;
>   double vars[NUMVARS] = {0};
>   double min = 0;
>   double max = 0;
> }

> [spec:cg3:def:math-parser.cg3.math-parser.eval-add-sub-fn]
> inline void MathParser::eval_add_sub(double& result)

> [spec:cg3:sem:math-parser.cg3.math-parser.eval-add-sub-fn]
> Additive precedence level (left-associative `+` and `-`). First
> evaluate a higher-precedence term into `result` via
> `eval_mul_div(result)`. Then loop while `token` is non-empty and its
> first char `op` is `'+'` or `'-'`: call `get_token()` to consume the
> operator, evaluate the next term into a local `temp` via
> `eval_mul_div(temp)`, then update `result = result - temp` for `-` or
> `result = result + temp` for `+`. Repeats left-to-right until the
> next token is not `+`/`-`.

> [spec:cg3:def:math-parser.cg3.math-parser.eval-assign-fn]
> inline void MathParser::eval_assign(double& result)

> [spec:cg3:sem:math-parser.cg3.math-parser.eval-assign-fn]
> Lowest-precedence level, handling an optional `VARIABLE = expression`
> assignment. If the current `tok_type` is `VARIABLE`: save the cursor
> `t_ptr = exp_ptr` and `temp_token = token`, and compute
> `slot = token[0] - 'A'` (0..25, from the variable's first letter).
> Call `get_token()` to look at the following token. If that token's
> first char is not `'='`, this was not an assignment: restore
> `exp_ptr = t_ptr`, `token = temp_token`, `tok_type = VARIABLE` (back
> up so the variable is re-read as a value) and fall through. If it is
> `'='`: call `get_token()` to consume `=`, evaluate the right-hand
> side with `eval_add_sub(result)`, store `vars[slot] = result`, and
> return immediately. In every non-assignment path, finish by calling
> `eval_add_sub(result)`. Quirk: `slot` is `token[0]-'A'` even for the
> names `MIN`/`MAX` (first letter `M` => slot 12), so `MIN=`/`MAX=`
> would overwrite `vars[12]` rather than the min/max members; a
> lowercase single-letter variable indexes out of bounds (`'a'-'A'`=32,
> but `vars` has only 26 slots).

> [spec:cg3:def:math-parser.cg3.math-parser.eval-exp-fn]
> inline void MathParser::eval_exp(double& result)

> [spec:cg3:sem:math-parser.cg3.math-parser.eval-exp-fn]
> Exponent precedence level (operator `^`). Evaluate a unary term into
> `result` via `eval_unary(result)`, then loop while `token` is
> non-empty and its first char is `'^'`: `get_token()`,
> `eval_unary(temp)`, `result = pow(result, temp)`. Because it is a
> `while` loop applied left-to-right, `a^b^c` is computed as
> `(a^b)^c` (left-associative), which differs from the conventional
> right-associative exponentiation.

> [spec:cg3:def:math-parser.cg3.math-parser.eval-fn]
> inline double MathParser::eval(UStringView exp)

> [spec:cg3:sem:math-parser.cg3.math-parser.eval-fn]
> Public entry point that evaluates the numeric expression `exp` (a
> UStringView over the whole expression text) and returns its value as
> a `double`. Steps: set `result = 0`; store `exp_ptr = exp` (the
> cursor used by `get_token`); call `get_token()` once to load the
> first token. If `token` is empty (empty/whitespace-only input),
> throw `std::runtime_error("Expression empty")`. Call
> `eval_assign(result)` to parse and evaluate the top-level expression
> (lowest precedence: assignment). After it returns, if `token` is
> still non-empty (leftover unconsumed input), throw
> `std::runtime_error("Syntax error")`. Otherwise return `result`.
> Note `vars`, `min`, `max` persist across calls on the same
> MathParser instance, so an assignment in one `eval` is visible to
> later ones.

> [spec:cg3:def:math-parser.cg3.math-parser.eval-func-fn]
> inline void MathParser::eval_func(double& result)

> [spec:cg3:sem:math-parser.cg3.math-parser.eval-func-fn]
> Highest-precedence level: a function call, a parenthesized
> sub-expression, a numeric literal, or a variable reference. Compute
> `isfunc = (tok_type == FUNCTION)`; if so, save the function name into
> `temp_token` and `get_token()` (advancing to what should be `(`).
> If the current `token[0] == '('`: `get_token()` to consume `(`,
> recursively evaluate the inner expression with
> `eval_add_sub(result)`, then require `token[0] == ')'` else throw
> `std::runtime_error("Unbalanced parentheses")`. If `isfunc`, dispatch
> on `temp_token` using the case-insensitive compare `ux_simplecasecmp`
> against, in order: `SIN`,`COS`,`TAN` (argument treated as degrees:
> e.g. `sin(M_PI/180.0*result)`); `ASIN`,`ACOS`,`ATAN` (result
> converted radians->degrees: `180.0/M_PI*asin(result)` etc.);
> `SINH`,`COSH`,`TANH`; `ASINH`,`ACOSH`,`ATANH`; `LN` (natural `log`);
> `LOG` (`log10`); `EXP`; `SQRT`; `SQR` (`result*result`); `ROUND`;
> `FLOOR`; if none match, throw
> `std::runtime_error("Unknown function")`. After applying the
> function (or if not a function), `get_token()` to consume `)`.
> Else (current token is not `(`): switch on `tok_type`. For
> `VARIABLE`: if the token is `MIN` (case-insensitive) set
> `result = min`, if `MAX` set `result = max`, otherwise
> `result = vars[token[0] - 'A']`; then `get_token()` and return. For
> `NUMBER`: copy the token's UChars into a fixed `char num[128]`
> buffer, null-terminate at `token.size()`, parse via
> `strtod(num, nullptr)`; if `errno == ERANGE` throw
> `std::runtime_error("Result did not fit in a double")`; then
> `get_token()` and return. For any other `tok_type` (including a
> `FUNCTION` name not followed by `(`): throw
> `std::runtime_error("Syntax error")`.
> Quirks: the `NUMBER` path writes into a 128-byte stack buffer with no
> length check (a token of 128+ UChars overflows); `errno` is not
> cleared before the `ERANGE` test, so it reflects any prior `ERANGE`;
> `vars[token[0]-'A']` assumes an uppercase A-Z letter, so a lowercase
> single-letter variable indexes out of bounds.

> [spec:cg3:def:math-parser.cg3.math-parser.eval-mul-div-fn]
> inline void MathParser::eval_mul_div(double& result)

> [spec:cg3:sem:math-parser.cg3.math-parser.eval-mul-div-fn]
> Multiplicative precedence level (left-associative `*` and `/`).
> Evaluate a higher-precedence factor into `result` via
> `eval_exp(result)`. Then loop while `token` is non-empty and its
> first char `op` is `'*'` or `'/'`: `get_token()`, `eval_exp(temp)`,
> then `result = result * temp` or `result = result / temp`. Division
> uses plain IEEE-754 `double` division with no zero check (so `/0`
> yields +/-inf or NaN).

> [spec:cg3:def:math-parser.cg3.math-parser.eval-unary-fn]
> inline void MathParser::eval_unary(double& result)

> [spec:cg3:sem:math-parser.cg3.math-parser.eval-unary-fn]
> Unary sign level. If the current `tok_type` is `DELIMITER` and
> `token[0]` is `'+'` or `'-'`, record it in `op` and `get_token()` to
> consume it (only a single leading sign is consumed). Then evaluate
> the operand via `eval_func(result)`. Finally, if `op == '-'`, negate
> `result = -result`; a leading `'+'` is a no-op (op left 0).

> [spec:cg3:def:math-parser.cg3.math-parser.get-token-fn]
> inline void MathParser::get_token()

> [spec:cg3:sem:math-parser.cg3.math-parser.get-token-fn]
> The lexer; reads the next token from `exp_ptr` into `token` and sets
> `tok_type`. Start by setting `token = exp_ptr` and `tok_type = 0`; if
> `exp_ptr` is empty, return immediately (leaving `token` empty). Skip
> leading whitespace by `remove_prefix(1)` for each
> `ISSPACE(exp_ptr[0])`. Then classify the first remaining char:
> - If `ISDELIM(exp_ptr[0])` (one of `( ) + - * / ^ % =`): set
>   `tok_type = DELIMITER`, `token = exp_ptr.substr(0,1)` (single
>   char), and advance one.
> - Else if `ISALPHA_C(exp_ptr[0])` (ASCII letter, code unit <255 and
>   `isalpha`): set `token` to `exp_ptr` up to the first of the
>   characters in `" +-/*%^=()"` (via `find_first_of`; whole rest if
>   none), advance past it, then skip following whitespace; set
>   `tok_type = FUNCTION` if the next char is `'('`, else `VARIABLE`.
> - Else if `ISDIGIT_C(exp_ptr[0])` or it is `'.'`: set `token` up to
>   the first of `" +-/*%^=()"`, advance past it, set
>   `tok_type = NUMBER`.
> - Otherwise `tok_type` stays 0 and `token` remains the (unadvanced)
>   remaining view.
> Finally, if `tok_type == VARIABLE`: if the token equals `MIN` or
> `MAX` (case-insensitive) it is accepted; else if `token.size() > 1`
> throw `std::runtime_error("Variables other than MIN and MAX must be
> 1 letter")`. Note the identifier/number stop-set includes `%` (a
> delimiter no eval level actually consumes), and a leading `.` starts
> a NUMBER token.

> [spec:cg3:def:math-parser.cg3.math-parser.math-parser-fn]
> MathParser(double min=0, double max=0) : min(min), max(max)

> [spec:cg3:sem:math-parser.cg3.math-parser.math-parser-fn]
> Constructor. Takes two optional `double` args `min` and `max` (both
> default 0) and copies them into the members `min`/`max` via the
> initializer list; body is empty. These two members supply the values
> returned when an expression references the identifiers `MIN`/`MAX`.
> All other members keep their in-class initializers: `tok_type = 0`,
> the 26-element `vars` array all zero, and the `exp_ptr`/`token`
> string-views empty.

> [spec:cg3:def:math-parser.cg3.math-parser.type-t]
> enum type_t:uint8_t {
>   DELIMITER = 1;
>   VARIABLE;
>   NUMBER;
>   FUNCTION;
> }

