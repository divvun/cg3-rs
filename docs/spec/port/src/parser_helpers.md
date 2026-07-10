# src/parser_helpers.hpp

> [spec:cg3:def:parser-helpers.cg3.parse-set-fn]
> Set* parseSet(const UChar* name, const UChar* p, State& state)

> [spec:cg3:sem:parser-helpers.cg3.parse-set-fn]
> Template free function (on the parser `State`) that resolves a set
> name to an existing `Set*`, lazily creating unification wrappers and
> on-demand single-tag LIST sets. `p` is only near-context for errors.
> Steps: `sh = hash_value(name)`. If `ux_isSetOp(name) != S_IGNORE` ->
> error "Found set operator where set name expected".
> Unification wrapper: if `name` begins with `$$` or `&&` AND has a
> third char (`name[2]`), let `wname = &name[2]`; if that tail matches
> the scanf pattern `%*u:%S` (an unsigned int, then `:`, then the rest)
> via `u_sscanf`, replace `wname` with the parsed-out remainder
> (so `$$123:Foo` wraps `Foo`). Compute `wrap = hash_value(wname)`;
> `wtmp = grammar->getSet(wrap)`; if null -> error "undefined set". If
> the grammar has no set for `sh` yet, allocate a new set `ns`, set its
> line/name(`name`), push `wtmp->hash` into `ns->sets`, set
> `ST_TAG_UNIFY` for `$$` or `ST_SET_UNIFY` for `&&`, and `addSet(ns)`.
> Alias resolution: if `grammar->set_alias` contains `sh`, replace
> `sh = set_alias[sh]`.
> Look up `tmp = grammar->getSet(sh)`. If found, return it. If not
> found: if either `state.strict_tags` or `state.list_tags` is
> non-empty, `parseTag(name, p, state)` the name and, if the resulting
> tag's `plain_hash` is in `strict_tags` or `list_tags`, build a
> single-tag LIST set on the fly (allocate `ns`, set line/name,
> `addTagToSet(tag, ns)`, `addSet(ns)`, return `ns`). Otherwise ->
> error "Attempted to reference undefined set". Errors are [[noreturn]]
> (throw).

> [spec:cg3:def:parser-helpers.cg3.parse-tag-fn]
> Tag* parseTag(const UChar* to, const UChar* p, State& state, bool unescape=true)

> [spec:cg3:sem:parser-helpers.cg3.parse-tag-fn]
> Template free function (parameterized on the concrete parser `State`)
> that turns a raw UTF-16 tag string `to` into a canonical `Tag*`,
> registered in the grammar. `p` is only near-context for errors;
> `unescape` (default true) controls backslash removal. Returns
> `state.addTag(tag)` (deduplicated).
> Validation first: if `to[0]==0` -> `state.error` empty-tag. If
> `to[0]=='('` -> error "cannot start with (". If
> `ux_isSetOp(to) != S_IGNORE` -> a warning (not fatal) that it looks
> like a set operator.
> Unicode-escape expansion (REGEX SITE): if `u_strstr(to, u"\\u")` (a
> literal backslash-u substring exists), rewrite `\uXXXX` / `\u{...}`
> into real code points. It uses a thread_local ICU `RegexMatcher`
> `rx_u` compiled once from the pattern source
> `\\u((?:[0-9a-fA-F]{4})|\{(?:[0-9a-fA-F]+)\})` with flags 0. i.e. a
> literal `\u` followed by EITHER exactly four hex digits OR `{` one-
> or-more hex digits `}`; group 1 captures that alternative (including
> the braces in the braced case). It opens a UText over `to`, resets
> the matcher, and loops with `rx_u.find()` — this is an UNANCHORED
> repeated search over the whole string, handling multiple escapes.
> For each match: `mb/me` = whole-match start/end, `sb/se` = group-1
> start/end; append the gap `to[l..mb)` to `tmp_tag`; if `to[sb]=='{'`
> do `++sb` and if `to[se-1]=='}'` do `--se` (strip braces); then parse
> `to[sb..se)` as big-endian hex into a `UChar32 uc` by iterating from
> the least-significant digit (`c = to[se-i-1]`, adding hex-value(c) <<
> (i*4), with a switch mapping a/A..f/F to 10..15 and default `c-'0'`).
> If `uc > 0xFFFF`, subtract 0x10000 and emit a UTF-16 surrogate pair
> (`(uc>>10)+0xD800`, `(uc&0x3FF)+0xDC00`); else emit one `UChar(uc)`.
> Set `l = me`. After the loop, if any match happened, append the tail
> `to+l` and repoint `to = tmp_tag.c_str()`. Rust regex-parity note:
> the `{4}` means EXACTLY four hex (a 5th hex after `\uXXXX` is left
> literal), the search must be repeated/unanchored, and the crate's
> pattern must escape the literal backslash and match on the bare `u`.
> Dedup: `thash = hash_value(to)`; if `single_tags` already has an
> entry for `thash` whose stored `tag` is non-empty and equals `to`,
> return that existing Tag*.
> Otherwise allocate a fresh Tag with `type = 0`. If `to[0]` is
> non-empty, process the body via `tmp = to`:
> - Consume any leading `^` chars, each setting `T_FAILFAST` and
>   advancing `tmp`. `length = u_strlen(tmp)` (asserted non-empty).
> - If `tmp` starts `T:` -> warning "misattempt of template usage".
> - Prefix scans, each an independent `if` in this order, stripping the
>   prefix off `tmp` and reducing `length`: `META:` -> T_META (+5);
>   `VAR:` -> T_VARIABLE, variable_hash=0 (+4); `LVAR:` ->
>   T_LOCAL_VARIABLE (+5); `SET:` -> T_SET (+4); `VSTR:` or `PSTR:` ->
>   if the leading char was `P` also set T_PRESERVE_ESC, set
>   T_VARSTRING|T_VSTR, `tmp += 5`, `tag->tag.assign(tmp)` RAW (no
>   suffix/escape handling), error if empty, then `goto
>   label_isVarstring` (skip all remaining body processing).
> - Textual/suffix detection: if `tmp[0]` is non-empty, the tag is not
>   a variable, and `tmp[0]` is one of `"`, `<`, `/`: save
>   `oldlength=length`, then strip trailing suffix letters from the end
>   (each at most once, guarded by not-already-set) decrementing
>   `length` per strip: `v`->T_VARSTRING, `r`->T_REGEXP,
>   `i`->T_CASE_INSENSITIVE, `l`->T_REGEXP|T_REGEXP_LINE,
>   `p`->T_VARSTRING|T_PRESERVE_ESC; break on any other char. Then if
>   `tmp[0]=='"' && tmp[length-1]=='"'`: T_WORDFORM if
>   `tmp[1]=='<' && tmp[length-2]=='>'` else T_BASEFORM. If the trimmed
>   body is fully delimited (`"..."`, `<...>`, or `/.../`) set
>   T_TEXTUAL; ELSE (mismatched delimiters) undo everything: clear the
>   T_VARSTRING/T_REGEXP/T_REGEXP_LINE/T_CASE_INSENSITIVE/T_WORDFORM/
>   T_BASEFORM bits and restore `length = oldlength` (so the suffix
>   letters become ordinary tag text).
> - Build `tag->tag` by appending `tmp[i]` for `i` in `[0, oldlength)`
>   while `tmp[i]!=0`; when `unescape` and `tmp[i]=='\\'`, skip the
>   backslash (`++i; --length`) so the next char is taken literally.
>   Error if the result is empty. `length` now tracks `tag->tag`'s
>   length.
> - T_REGEXP_LINE `__` substitution (provisional/ToDo): while
>   `tag->tag` contains `__`, replace it with the 15-unit regex
>   `(?:^|$| | .+? )`, adjusting `length` by `size(rx)-size(uu)` each
>   time.
> - regex_tags scan (REGEX SITE): for each compiled `iter` in
>   `grammar->regex_tags`, `uregex_setText(iter, tag->tag, ...)` (error
>   on non-zero status), then `uregex_find(iter, -1, &status)` — an
>   UNANCHORED whole-string search; if it matches, set T_TEXTUAL.
> - icase_tags scan: for each `iter` in `grammar->icase_tags`, if
>   `ux_strCaseCompare(tag->tag, iter->tag)` (full case-fold equality)
>   set T_TEXTUAL.
> - Variable split: if type has T_VARIABLE|T_LOCAL_VARIABLE, find `=`
>   in `tag->tag`. If found: `comparison_op = OP_EQUALS`;
>   `variable_hash = parseTag(text-after-'=', p, state, false)->hash`;
>   temporarily NUL the `=` and `comparison_hash =
>   parseTag(text-before-'=', ...)->hash`; restore the `=`. Else
>   `comparison_hash = parseTag(whole, ...)->hash`. Otherwise (non-
>   variable) `comparison_hash = hash_value(tag->tag)`.
> - Numeric `<...>`: if `tag->tag[0]=='<' && tag->tag[length-1]=='>'`
>   and none of T_CASE_INSENSITIVE/T_REGEXP/T_REGEXP_LINE/T_VARSTRING
>   are set, call `tag->parseNumeric(true)`.
> - Special-name recognition (exact-equality on the final `tag->tag`):
>   `*` -> T_ANY; `_LEFT_` -> T_PAR_LEFT; `_RIGHT_` -> T_PAR_RIGHT;
>   `_ENCL_` -> T_ENCL; `_TARGET_` -> T_TARGET; `_MARK_` -> T_MARK;
>   `_ATTACHTO_` -> T_ATTACHTO; `_SAME_BASIC_` -> T_SAME_BASIC;
>   `_C1_`.._C9_` -> T_CONTEXT with `context_ref_pos` = 1..9
>   respectively (the STR_UU_* / STR_ASTERIK constants).
> - Regex compile (REGEX SITE) if T_REGEXP: if `tag->tag` equals one of
>   STR_RXTEXT_ANY/STR_RXBASE_ANY/STR_RXWORD_ANY -> set T_REGEXP_ANY,
>   clear T_REGEXP (the special "match anything" regex). Else build the
>   pattern `rt`: if `tag->tag` begins and ends with `/`, `rt =
>   tag->tag.substr(1, length-2)` (strip the slashes, pattern used
>   UNANCHORED as written); else `rt = '^' + tag->tag + '$'` (anchored
>   full-string match). Compile with `uregex_open(rt, size,
>   UREGEX_CASE_INSENSITIVE if T_CASE_INSENSITIVE else 0, ...)`, error
>   on non-zero status, store into `tag->regexp`.
> - After compile: if type has T_CASE_INSENSITIVE|T_REGEXP and
>   `tag->tag` begins/ends with `/`, strip the surrounding slashes from
>   the stored `tag->tag` (resize -1, erase front) — the compiled regex
>   already captured the content.
> - `label_isVarstring:` is the join point for the VSTR/PSTR early
>   jump.
> After the body: clear T_SPECIAL, then if `type & MASK_TAG_SPECIAL`
> set T_SPECIAL. If T_VARSTRING is combined with any of
> T_REGEXP/T_REGEXP_ANY/T_VARIABLE/T_LOCAL_VARIABLE/T_META -> error
> "cannot mix varstring with any other special feature". If the final
> `tag->tag` differs from the original `to` (`USV(tag->tag) != to`),
> store `tag->tag_raw = to` (original spelling). Return
> `state.addTag(tag)`. Every `state.error(...)` is [[noreturn]] (throws
> to abort the current construct).

