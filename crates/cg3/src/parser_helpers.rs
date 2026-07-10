//! Port of `src/parser_helpers.hpp` — the shared free parser helpers
//! `parseTag` / `parseSet` (spec `docs/spec/port/src/parser_helpers.md`).
//!
//! Literal, bug-for-bug 1:1 translation (Wave 2, translate pass).
//!
//! ## Template `State` → [`ParseTagState`] trait
//! The C++ helpers are `template<typename State>` free functions, parameterised
//! on the concrete state so they can call `state.error(...)`,
//! `state.addTag(...)`, `state.get_grammar()`, and read `state.strict_tags` /
//! `state.list_tags`. The C++ instantiates them with BOTH `TextualParser`
//! (grammar compile time) and `GrammarApplicator` (runtime varstring tags, via
//! `GrammarApplicator::addTag`). `parse_tag` is therefore generic over the small
//! [`ParseTagState`] trait, implemented by both. `parse_set` is only ever
//! instantiated with `TextualParser` (it reads `strict_tags` / `list_tags` and
//! allocates sets), so it stays a plain free function on the concrete parser.
//!
//! NOTE on error semantics: `TextualParser::error` is fatal (panic-unwind /
//! quit), but `GrammarApplicator::error` just prints to `ux_stderr` and RETURNS.
//! The trait's `error_near` therefore returns `()`; the parser's impl diverges
//! anyway, and the code after each error site matches the C++ fall-through of
//! the applicator instantiation.
//!
//! ## Pointer / near-context model
//! The C++ `const UChar* p` argument is *only* near-context for error messages.
//! It is ported as `near: &[char]` — the tail of the source buffer starting at
//! the error position (what `ux_bufcpy` copies its 20 chars from). `to` / `name`
//! are the token being parsed (a UTF-8 `&str`), decoupled from `near`.
//!
//! ## Regex (`\uXXXX` expansion + regex compile)
//! ICU `RegexMatcher rx_u` → a `regex::Regex` compiled once via [`LazyLock`].
//! ICU `uregex_open` for a `T_REGEXP` tag → `regex::Regex::new` (Unicode by
//! default). Parity with ICU regex syntax is a known risk (documented at each
//! site); an unanchored `/.../` pattern is used verbatim, a bare pattern is
//! wrapped `^…$`, and `T_CASE_INSENSITIVE` becomes a leading `(?i)`.

#![allow(clippy::needless_range_loop)]

use std::sync::LazyLock;

use regex::Regex;

use crate::arena::{SetId, TagId};
use crate::grammar::Grammar;
use crate::inlines::hash_value_ustring;
use crate::set::{ST_SET_UNIFY, ST_TAG_UNIFY};
use crate::tag::{
    C_OPS, MASK_TAG_SPECIAL, T_ANY, T_ATTACHTO, T_BASEFORM, T_CASE_INSENSITIVE, T_CONTEXT, T_ENCL,
    T_FAILFAST, T_LOCAL_VARIABLE, T_MARK, T_META, T_PAR_LEFT, T_PAR_RIGHT, T_PRESERVE_ESC, T_REGEXP,
    T_REGEXP_ANY, T_REGEXP_LINE, T_SAME_BASIC, T_SET, T_SPECIAL, T_TARGET, T_TEXTUAL, T_VARIABLE,
    T_VARSTRING, T_VSTR, T_WORDFORM, Tag,
};
use crate::textual_parser::TextualParser;
use crate::uextras::{S_IGNORE, ux_is_set_op};

// Local `STR_*` constants (their canonical home is `Strings.hpp`, which the
// `strings` port has not yet grown; reproduced verbatim, same precedent as
// `grammar.rs`).
const STR_ASTERIK: &str = "*";
const STR_UU_LEFT: &str = "_LEFT_";
const STR_UU_RIGHT: &str = "_RIGHT_";
const STR_UU_ENCL: &str = "_ENCL_";
const STR_UU_TARGET: &str = "_TARGET_";
const STR_UU_MARK: &str = "_MARK_";
const STR_UU_ATTACHTO: &str = "_ATTACHTO_";
const STR_UU_SAME_BASIC: &str = "_SAME_BASIC_";
const STR_UU_C1: &str = "_C1_";
const STR_UU_C2: &str = "_C2_";
const STR_UU_C3: &str = "_C3_";
const STR_UU_C4: &str = "_C4_";
const STR_UU_C5: &str = "_C5_";
const STR_UU_C6: &str = "_C6_";
const STR_UU_C7: &str = "_C7_";
const STR_UU_C8: &str = "_C8_";
const STR_UU_C9: &str = "_C9_";
const STR_RXTEXT_ANY: &str = "<.*>";
const STR_RXBASE_ANY: &str = "\".*\"";
const STR_RXWORD_ANY: &str = "\"<.*>\"";

/// C++ `thread_local RegexMatcher rx_u(...)`. Matches a literal `\u` followed by
/// EITHER exactly four hex digits OR `{` one-or-more hex digits `}`; group 1
/// captures that alternative (braces included). Compiled once.
static RX_U: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\\u((?:[0-9a-fA-F]{4})|\{(?:[0-9a-fA-F]+)\})").unwrap());

/// NUL-terminator-aware char access: index `>= len` reads as `'\0'`, matching the
/// C++ `UChar*` reads at/after the terminator that the short-circuits guard.
#[inline]
fn cat(chars: &[char], i: usize) -> char {
    if i < chars.len() { chars[i] } else { '\0' }
}

/// The C++ `template<typename State>` surface `parseTag` actually uses:
/// `state.get_grammar()` (reads only), `state.addTag(Tag*)` (interning entry —
/// `Grammar::addTag` for the parser, `GrammarApplicator::addTag(Tag*)`'s
/// seed-probe for the applicator), `state.error(...)` and `state.filebase` /
/// `state.ux_stderr` for diagnostics.
pub trait ParseTagState {
    fn grammar(&self) -> &Grammar;
    /// `state.filebase` — diagnostics prefix (`nullptr` on the applicator → "").
    fn filebase(&self) -> &str;
    /// `state.error(...)`: FATAL on `TextualParser` (diverges), non-fatal
    /// print-and-return on `GrammarApplicator` — hence `()` and not `!`.
    fn error_near(&mut self, near: &[char]);
    /// `state.addTag(tag)` — intern the freshly-built tag, return canonical id.
    fn add_tag(&mut self, tag: Tag) -> TagId;
}

impl ParseTagState for TextualParser {
    fn grammar(&self) -> &Grammar {
        &self.grammar
    }
    fn filebase(&self) -> &str {
        &self.filebase
    }
    fn error_near(&mut self, near: &[char]) {
        // Inherent `TextualParser::error_near` is `-> !` (fatal), per the C++.
        TextualParser::error_near(self, near)
    }
    fn add_tag(&mut self, tag: Tag) -> TagId {
        TextualParser::add_tag(self, tag)
    }
}

// [spec:cg3:def:parser-helpers.cg3.parse-tag-fn]
// [spec:cg3:sem:parser-helpers.cg3.parse-tag-fn]
pub fn parse_tag<S: ParseTagState>(to: &str, near: &[char], state: &mut S, unescape: bool) -> TagId {
    // Validation first.
    let to0 = to.chars().next().unwrap_or('\0');
    if to0 == '\0' {
        state.error_near(near); // "Empty tag ... Forgot to fill in a ()?"
    }
    if to0 == '(' {
        state.error_near(near); // "Tag ... cannot start with ("
    }
    if ux_is_set_op(to) != S_IGNORE {
        // Warning (not fatal): looks like a set operator.
        eprintln!("{}: Warning: Tag '{}' looks like a set operator.", state.filebase(), to);
    }

    // Parse \uxxxx and \u{x} as Unicode code points.
    let mut to_owned = to.to_string();
    if to_owned.contains("\\u") {
        let re = &*RX_U;
        let src = to_owned.clone();
        let mut tmp = String::new();
        let mut l = 0usize; // byte offset
        let mut did = false;
        for m in re.captures_iter(&src) {
            let whole = m.get(0).unwrap();
            let grp = m.get(1).unwrap();
            let mb = whole.start();
            let me_ = whole.end();
            let mut sb = grp.start();
            let mut se = grp.end();
            tmp.push_str(&src[l..mb]);
            let b = src.as_bytes();
            if b[sb] == b'{' {
                sb += 1;
            }
            if b[se - 1] == b'}' {
                se -= 1;
            }
            // Big-endian hex → code point. (UTF-8 port: a code point > 0xFFFF is
            // one `char`, not a UTF-16 surrogate pair.)
            let uc = u32::from_str_radix(&src[sb..se], 16).unwrap_or(0);
            tmp.push(char::from_u32(uc).unwrap_or('\u{FFFD}'));
            l = me_;
            did = true;
        }
        if did {
            tmp.push_str(&src[l..]);
            to_owned = tmp;
        }
    }

    // Dedup: `single_tags[thash]->tag == to` → return existing.
    let thash = hash_value_ustring(&to_owned, 0);
    {
        let g = state.grammar();
        let it = g.single_tags.find(thash);
        if it != g.single_tags.end() {
            let tid = it.get().1;
            let existing = &g.single_tags_list[tid.0];
            if !existing.tag.is_empty() && existing.tag == to_owned {
                return tid;
            }
        }
    }

    let mut tag = Tag::default();
    tag.r#type = 0;

    let to_chars: Vec<char> = to_owned.chars().collect();

    if !to_chars.is_empty() {
        // Consume leading `^` (T_FAILFAST) chars.
        let mut tmp_off = 0usize;
        while cat(&to_chars, tmp_off) != '\0' && cat(&to_chars, tmp_off) == '^' {
            tag.r#type |= T_FAILFAST;
            tmp_off += 1;
        }
        // length = u_strlen(tmp)
        let mut length: usize = to_chars.len().saturating_sub(tmp_off);
        debug_assert!(length != 0, "parseTag() will not work with empty strings.");

        // tmp[i] == to_chars[tmp_off + i]
        let tget = |off: usize, i: usize, chars: &[char]| -> char { cat(chars, off + i) };

        if tget(tmp_off, 0, &to_chars) == 'T' && tget(tmp_off, 1, &to_chars) == ':' {
            eprintln!("{}: Warning: Tag looks like a misattempt of template usage.", state.filebase());
        }

        // Prefix scans (independent ifs, source order).
        if tget(tmp_off, 0, &to_chars) == 'M'
            && tget(tmp_off, 1, &to_chars) == 'E'
            && tget(tmp_off, 2, &to_chars) == 'T'
            && tget(tmp_off, 3, &to_chars) == 'A'
            && tget(tmp_off, 4, &to_chars) == ':'
        {
            tag.r#type |= T_META;
            tmp_off += 5;
            length -= 5;
        }
        if tget(tmp_off, 0, &to_chars) == 'V'
            && tget(tmp_off, 1, &to_chars) == 'A'
            && tget(tmp_off, 2, &to_chars) == 'R'
            && tget(tmp_off, 3, &to_chars) == ':'
        {
            tag.r#type |= T_VARIABLE;
            tag.dep_parent = 0; // variable_hash = 0 (union alias)
            tmp_off += 4;
            length -= 4;
        }
        if tget(tmp_off, 0, &to_chars) == 'L'
            && tget(tmp_off, 1, &to_chars) == 'V'
            && tget(tmp_off, 2, &to_chars) == 'A'
            && tget(tmp_off, 3, &to_chars) == 'R'
            && tget(tmp_off, 4, &to_chars) == ':'
        {
            tag.r#type |= T_LOCAL_VARIABLE;
            tmp_off += 5;
            length -= 5;
        }
        if tget(tmp_off, 0, &to_chars) == 'S'
            && tget(tmp_off, 1, &to_chars) == 'E'
            && tget(tmp_off, 2, &to_chars) == 'T'
            && tget(tmp_off, 3, &to_chars) == ':'
        {
            tag.r#type |= T_SET;
            tmp_off += 4;
            length -= 4;
        }
        let mut jumped_varstring = false;
        if (tget(tmp_off, 0, &to_chars) == 'V' || tget(tmp_off, 0, &to_chars) == 'P')
            && tget(tmp_off, 1, &to_chars) == 'S'
            && tget(tmp_off, 2, &to_chars) == 'T'
            && tget(tmp_off, 3, &to_chars) == 'R'
            && tget(tmp_off, 4, &to_chars) == ':'
        {
            if tget(tmp_off, 0, &to_chars) == 'P' {
                tag.r#type |= T_PRESERVE_ESC;
            }
            tag.r#type |= T_VARSTRING;
            tag.r#type |= T_VSTR;
            tmp_off += 5;

            // tag->tag.assign(tmp) RAW.
            tag.tag = to_chars[tmp_off.min(to_chars.len())..].iter().collect();
            if tag.tag.is_empty() {
                state.error_near(near); // "resulted in an empty tag"
            }
            jumped_varstring = true;
        }

        if !jumped_varstring {
            // Textual / suffix detection.
            if tget(tmp_off, 0, &to_chars) != '\0'
                && (tag.r#type & (T_VARIABLE | T_LOCAL_VARIABLE)) == 0
                && (tget(tmp_off, 0, &to_chars) == '"'
                    || tget(tmp_off, 0, &to_chars) == '<'
                    || tget(tmp_off, 0, &to_chars) == '/')
            {
                let oldlength = length;

                // Strip trailing suffix letters r, i, v, l, p — max one of each.
                loop {
                    let last = tget(tmp_off, length - 1, &to_chars);
                    if !(last == 'i' || last == 'r' || last == 'v' || last == 'l' || last == 'p') {
                        break;
                    }
                    if (tag.r#type & T_VARSTRING) == 0 && last == 'v' {
                        tag.r#type |= T_VARSTRING;
                        length -= 1;
                        continue;
                    }
                    if (tag.r#type & T_REGEXP) == 0 && last == 'r' {
                        tag.r#type |= T_REGEXP;
                        length -= 1;
                        continue;
                    }
                    if (tag.r#type & T_CASE_INSENSITIVE) == 0 && last == 'i' {
                        tag.r#type |= T_CASE_INSENSITIVE;
                        length -= 1;
                        continue;
                    }
                    if (tag.r#type & T_REGEXP_LINE) == 0 && last == 'l' {
                        tag.r#type |= T_REGEXP;
                        tag.r#type |= T_REGEXP_LINE;
                        length -= 1;
                        continue;
                    }
                    if (tag.r#type & T_PRESERVE_ESC) == 0 && last == 'p' {
                        tag.r#type |= T_VARSTRING;
                        tag.r#type |= T_PRESERVE_ESC;
                        length -= 1;
                        continue;
                    }
                    break;
                }

                let first = tget(tmp_off, 0, &to_chars);
                let last = tget(tmp_off, length - 1, &to_chars);
                if first == '"' && last == '"' {
                    if tget(tmp_off, 1, &to_chars) == '<' && tget(tmp_off, length - 2, &to_chars) == '>' {
                        tag.r#type |= T_WORDFORM;
                    } else {
                        tag.r#type |= T_BASEFORM;
                    }
                }

                if (first == '"' && last == '"')
                    || (first == '<' && last == '>')
                    || (first == '/' && last == '/')
                {
                    tag.r#type |= T_TEXTUAL;
                } else {
                    tag.r#type &= !T_VARSTRING;
                    tag.r#type &= !T_REGEXP;
                    tag.r#type &= !T_REGEXP_LINE;
                    tag.r#type &= !T_CASE_INSENSITIVE;
                    tag.r#type &= !T_WORDFORM;
                    tag.r#type &= !T_BASEFORM;
                    length = oldlength;
                }
            }

            // Build tag->tag, honoring backslash unescape.
            let oldlength = length;
            let mut new_length = length;
            let mut built = String::new();
            let mut i = 0usize;
            while i < oldlength && tget(tmp_off, i, &to_chars) != '\0' {
                if unescape && tget(tmp_off, i, &to_chars) == '\\' {
                    i += 1;
                    new_length -= 1;
                }
                if tget(tmp_off, i, &to_chars) == '\0' {
                    break;
                }
                built.push(tget(tmp_off, i, &to_chars));
                i += 1;
            }
            tag.tag = built;
            length = new_length;
            if tag.tag.is_empty() {
                state.error_near(near);
            }

            // ToDo: T_REGEXP_LINE `__` substitution.
            if tag.r#type & T_REGEXP_LINE != 0 {
                while let Some(pos) = tag.tag.find("__") {
                    tag.tag.replace_range(pos..pos + 2, "(?:^|$| | .+? )");
                    length += 15 - 2;
                }
            }

            // regex_tags scan (empty during textual parse; populated at runtime
            // by GrammarApplicator::addTag) — unanchored is_match.
            let regex_ids: Vec<TagId> = state.grammar().regex_tags.iter().copied().collect();
            for tid in regex_ids {
                if let Some(re) = &state.grammar().single_tags_list[tid.0].regexp {
                    if re.is_match(&tag.tag) {
                        tag.r#type |= T_TEXTUAL;
                    }
                }
            }
            // icase_tags scan (empty during textual parse).
            let icase_ids: Vec<TagId> = state.grammar().icase_tags.iter().copied().collect();
            for tid in icase_ids {
                if ux_str_case_compare(&tag.tag, &state.grammar().single_tags_list[tid.0].tag) {
                    tag.r#type |= T_TEXTUAL;
                }
            }

            // Variable split.
            if tag.r#type & (T_VARIABLE | T_LOCAL_VARIABLE) != 0 {
                let tag_tag = tag.tag.clone();
                if let Some(bpos) = tag_tag.find('=') {
                    tag.comparison_op = C_OPS::OP_EQUALS;
                    let after: String = tag_tag[bpos + 1..].to_string();
                    let vh = {
                        let t = parse_tag(&after, near, state, false);
                        state.grammar().single_tags_list[t.0].hash
                    };
                    tag.dep_parent = vh; // variable_hash (union alias)
                    let before: String = tag_tag[..bpos].to_string();
                    let ch = {
                        let t = parse_tag(&before, near, state, false);
                        state.grammar().single_tags_list[t.0].hash
                    };
                    tag.comparison_hash = ch;
                } else {
                    let ch = {
                        let t = parse_tag(&tag_tag, near, state, false);
                        state.grammar().single_tags_list[t.0].hash
                    };
                    tag.comparison_hash = ch;
                }
            } else {
                tag.comparison_hash = hash_value_ustring(&tag.tag, 0);
            }

            // Numeric `<...>`.
            {
                let tchars: Vec<char> = tag.tag.chars().collect();
                if cat(&tchars, 0) == '<'
                    && cat(&tchars, length.wrapping_sub(1)) == '>'
                    && (tag.r#type
                        & (T_CASE_INSENSITIVE | T_REGEXP | T_REGEXP_LINE | T_VARSTRING))
                        == 0
                {
                    tag.parse_numeric(true);
                }
            }

            // Special-name recognition (exact equality on final tag->tag).
            if tag.tag == STR_ASTERIK {
                tag.r#type |= T_ANY;
            } else if tag.tag == STR_UU_LEFT {
                tag.r#type |= T_PAR_LEFT;
            } else if tag.tag == STR_UU_RIGHT {
                tag.r#type |= T_PAR_RIGHT;
            } else if tag.tag == STR_UU_ENCL {
                tag.r#type |= T_ENCL;
            } else if tag.tag == STR_UU_TARGET {
                tag.r#type |= T_TARGET;
            } else if tag.tag == STR_UU_MARK {
                tag.r#type |= T_MARK;
            } else if tag.tag == STR_UU_ATTACHTO {
                tag.r#type |= T_ATTACHTO;
            } else if tag.tag == STR_UU_SAME_BASIC {
                tag.r#type |= T_SAME_BASIC;
            } else if tag.tag == STR_UU_C1 {
                tag.r#type |= T_CONTEXT;
                tag.dep_parent = 1; // context_ref_pos (union alias)
            } else if tag.tag == STR_UU_C2 {
                tag.r#type |= T_CONTEXT;
                tag.dep_parent = 2;
            } else if tag.tag == STR_UU_C3 {
                tag.r#type |= T_CONTEXT;
                tag.dep_parent = 3;
            } else if tag.tag == STR_UU_C4 {
                tag.r#type |= T_CONTEXT;
                tag.dep_parent = 4;
            } else if tag.tag == STR_UU_C5 {
                tag.r#type |= T_CONTEXT;
                tag.dep_parent = 5;
            } else if tag.tag == STR_UU_C6 {
                tag.r#type |= T_CONTEXT;
                tag.dep_parent = 6;
            } else if tag.tag == STR_UU_C7 {
                tag.r#type |= T_CONTEXT;
                tag.dep_parent = 7;
            } else if tag.tag == STR_UU_C8 {
                tag.r#type |= T_CONTEXT;
                tag.dep_parent = 8;
            } else if tag.tag == STR_UU_C9 {
                tag.r#type |= T_CONTEXT;
                tag.dep_parent = 9;
            }

            // Regex compile.
            if tag.r#type & T_REGEXP != 0 {
                if tag.tag == STR_RXTEXT_ANY || tag.tag == STR_RXBASE_ANY || tag.tag == STR_RXWORD_ANY {
                    tag.r#type |= T_REGEXP_ANY;
                    tag.r#type &= !T_REGEXP;
                } else {
                    let tchars: Vec<char> = tag.tag.chars().collect();
                    let rt: String = if cat(&tchars, 0) == '/' && cat(&tchars, length - 1) == '/' {
                        // strip the slashes, pattern used UNANCHORED as written
                        tchars[1..length - 1].iter().collect()
                    } else {
                        let mut s = String::from("^");
                        s.push_str(&tag.tag);
                        s.push('$');
                        s
                    };
                    let pat = if tag.r#type & T_CASE_INSENSITIVE != 0 {
                        format!("(?i){rt}")
                    } else {
                        rt
                    };
                    match Regex::new(&pat) {
                        Ok(re) => tag.regexp = Some(re),
                        Err(_) => state.error_near(near), // uregex_open returned error
                    }
                }
            }
            if tag.r#type & (T_CASE_INSENSITIVE | T_REGEXP) != 0 {
                let tchars: Vec<char> = tag.tag.chars().collect();
                if cat(&tchars, 0) == '/' && cat(&tchars, length - 1) == '/' {
                    // resize(-1) + erase(begin()) → drop first and last char.
                    let inner: String = tchars[1..tchars.len() - 1].iter().collect();
                    tag.tag = inner;
                }
            }
        }
        // label_isVarstring:
    }

    tag.r#type &= !T_SPECIAL;
    if tag.r#type & MASK_TAG_SPECIAL != 0 {
        tag.r#type |= T_SPECIAL;
    }

    if tag.r#type & T_VARSTRING != 0
        && tag.r#type & (T_REGEXP | T_REGEXP_ANY | T_VARIABLE | T_LOCAL_VARIABLE | T_META) != 0
    {
        state.error_near(near); // "cannot mix varstring with any other special feature"
    }

    if tag.tag != to_owned {
        tag.tag_raw = to_owned;
    }

    state.add_tag(tag)
}

// [spec:cg3:def:parser-helpers.cg3.parse-set-fn]
// [spec:cg3:sem:parser-helpers.cg3.parse-set-fn]
pub fn parse_set(name: &str, near: &[char], state: &mut TextualParser) -> SetId {
    let mut sh = hash_value_ustring(name, 0);

    if ux_is_set_op(name) != S_IGNORE {
        state.error_near(near); // "Found set operator where set name expected"
    }

    let nchars: Vec<char> = name.chars().collect();
    if ((cat(&nchars, 0) == '$' && cat(&nchars, 1) == '$')
        || (cat(&nchars, 0) == '&' && cat(&nchars, 1) == '&'))
        && cat(&nchars, 2) != '\0'
    {
        // wname = &name[2]; if it matches `%*u:%S`, wname = the remainder.
        let tail: String = nchars[2..].iter().collect();
        let wname: String = scan_star_u_colon_s(&tail).unwrap_or(tail);
        let wrap = hash_value_ustring(&wname, 0);
        let wtmp = match state.grammar.get_set(wrap) {
            Some(s) => s,
            None => state.error_near(near), // "reference undefined set"
        };
        let tmp = state.grammar.get_set(sh);
        if tmp.is_none() {
            let ns = state.grammar.allocate_set();
            state.grammar.sets_list[ns.0].line = state.grammar.lines;
            state.grammar.sets_list[ns.0].name = name.to_string();
            let wtmp_hash = state.grammar.sets_list[wtmp.0].hash;
            state.grammar.sets_list[ns.0].sets.push(wtmp_hash);
            if cat(&nchars, 0) == '$' && cat(&nchars, 1) == '$' {
                state.grammar.sets_list[ns.0].r#type |= ST_TAG_UNIFY;
            } else if cat(&nchars, 0) == '&' && cat(&nchars, 1) == '&' {
                state.grammar.sets_list[ns.0].r#type |= ST_SET_UNIFY;
            }
            state.grammar.add_set(ns);
        }
    }

    // Alias resolution.
    if state.grammar.set_alias.contains(sh) {
        let it = state.grammar.set_alias.find(sh);
        sh = it.get().1;
    }

    let tmp = state.grammar.get_set(sh);
    if let Some(tmp) = tmp {
        return tmp;
    }
    // Not found.
    if !state.strict_tags.empty() || !state.list_tags.empty() {
        let tag = parse_tag(name, near, state, true);
        let plain = state.grammar.single_tags_list[tag.0].plain_hash;
        if state.strict_tags.count(plain) != 0 || state.list_tags.count(plain) != 0 {
            let ns = state.grammar.allocate_set();
            state.grammar.sets_list[ns.0].line = state.grammar.lines;
            state.grammar.sets_list[ns.0].name = name.to_string();
            state.grammar.add_tag_to_set(tag, ns);
            let ns = state.grammar.add_set(ns);
            return ns;
        }
    }
    state.error_near(near) // "Attempted to reference undefined set"
}

/// `u_sscanf(wname, "%*u:%S", &out) == 1`: skip an unsigned int, require `:`,
/// return the remaining (whitespace-delimited) string. `%*u` is suppressed, so
/// the return count is 0 or 1 — `Some(rest)` iff a uint, then `:`, then a
/// non-empty non-whitespace `%S` followed.
fn scan_star_u_colon_s(s: &str) -> Option<String> {
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0usize;
    while i < chars.len() && chars[i].is_whitespace() {
        i += 1;
    }
    // %u: optional sign then digits (must read at least one digit).
    if i < chars.len() && (chars[i] == '+' || chars[i] == '-') {
        i += 1;
    }
    let mut any = false;
    while i < chars.len() && chars[i].is_ascii_digit() {
        i += 1;
        any = true;
    }
    if !any {
        return None;
    }
    // literal ':'
    if i >= chars.len() || chars[i] != ':' {
        return None;
    }
    i += 1;
    // %S: a whitespace-delimited string (skip leading whitespace, read to ws).
    while i < chars.len() && chars[i].is_whitespace() {
        i += 1;
    }
    let start = i;
    while i < chars.len() && !chars[i].is_whitespace() {
        i += 1;
    }
    if start == i {
        return None; // %S not assigned
    }
    Some(chars[start..i].iter().collect())
}

/// `uextras.hpp` `ux_strCaseCompare` — full case-fold equality (Unicode
/// lowercase-fold approximation; ICU parity risk for non-ASCII).
fn ux_str_case_compare(a: &str, b: &str) -> bool {
    a.chars().flat_map(char::to_lowercase).eq(b.chars().flat_map(char::to_lowercase))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parser() -> TextualParser {
        TextualParser::new(Grammar::default(), false)
    }

    /// The tag text of a parsed tag id.
    fn tag_text(p: &TextualParser, id: TagId) -> String {
        p.grammar.single_tags_list[id.0].tag.clone()
    }
    fn tag_type(p: &TextualParser, id: TagId) -> u32 {
        p.grammar.single_tags_list[id.0].r#type
    }

    // `parse_tag` builds/interns a Tag from source text: it classifies wordform
    // vs baseform vs plain tags, consumes the `^` failfast prefix, recognizes the
    // `*` special (T_ANY), and dedups (same text -> same TagId). Drives the whole
    // helper on non-error inputs (no `error_near` panic).
    // [spec:cg3:sem:parser-helpers.cg3.parse-tag-fn/test]
    #[test]
    fn parse_tag_classifies_and_dedups() {
        let mut p = parser();
        let near: Vec<char> = "noun".chars().collect();

        // Plain tag: text preserved, no textual flags.
        let t = parse_tag("noun", &near, &mut p, true);
        assert_eq!(tag_text(&p, t), "noun");
        assert_eq!(tag_type(&p, t) & T_TEXTUAL, 0);

        // Dedup: same source text returns the same interned id.
        let t2 = parse_tag("noun", &near, &mut p, true);
        assert_eq!(t, t2, "identical tag text dedups to one id");

        // Baseform: "lemma" (quoted) -> T_BASEFORM | T_TEXTUAL, text keeps quotes.
        let bsrc = "\"lemma\"";
        let bnear: Vec<char> = bsrc.chars().collect();
        let b = parse_tag(bsrc, &bnear, &mut p, true);
        assert_ne!(tag_type(&p, b) & T_BASEFORM, 0, "quoted lemma is a baseform");
        assert_ne!(tag_type(&p, b) & T_TEXTUAL, 0);
        assert_eq!(tag_text(&p, b), "\"lemma\"");

        // Wordform: "\"<word>\"" -> T_WORDFORM | T_TEXTUAL.
        let wsrc = "\"<word>\"";
        let wnear: Vec<char> = wsrc.chars().collect();
        let w = parse_tag(wsrc, &wnear, &mut p, true);
        assert_ne!(tag_type(&p, w) & T_WORDFORM, 0, "\"<...>\" is a wordform");
        assert_ne!(tag_type(&p, w) & T_TEXTUAL, 0);

        // Failfast prefix `^`: sets T_FAILFAST (and is special).
        let fsrc = "^bad";
        let fnear: Vec<char> = fsrc.chars().collect();
        let f = parse_tag(fsrc, &fnear, &mut p, true);
        assert_ne!(tag_type(&p, f) & T_FAILFAST, 0, "leading ^ is failfast");

        // `*` special -> T_ANY.
        let star: Vec<char> = "*".chars().collect();
        let any = parse_tag("*", &star, &mut p, true);
        assert_ne!(tag_type(&p, any) & T_ANY, 0, "* is T_ANY");
    }

    // `parse_set` resolves a set NAME to a SetId. Via the `list_tags` path: when
    // the name isn't a known set but its plain-hash is registered in `list_tags`,
    // parse_set parses it as a tag, allocates a fresh single-tag set, registers
    // it, and returns it. A second call with the same name resolves the now-known
    // set directly (get_set hit). Drives parse_set (and, transitively, parse_tag).
    // [spec:cg3:sem:parser-helpers.cg3.parse-set-fn/test]
    #[test]
    fn parse_set_allocates_from_list_tag_then_resolves() {
        let mut p = parser();
        let name = "noun";
        let near: Vec<char> = name.chars().collect();

        // Seed list_tags with this tag's plain-hash so parse_set's not-found path
        // will synthesize a set for it.
        let plain = {
            let t = parse_tag(name, &near, &mut p, true);
            p.grammar.single_tags_list[t.0].plain_hash
        };
        p.list_tags.insert(plain);

        // First resolution: allocates + registers a new single-tag set.
        let s1 = parse_set(name, &near, &mut p);
        assert_eq!(p.grammar.sets_list[s1.0].name, name);

        // Second resolution: the set is now known, so get_set returns it directly
        // — same SetId, no duplicate allocation.
        let s2 = parse_set(name, &near, &mut p);
        assert_eq!(s1, s2, "second parse_set resolves the existing set");
    }
}
