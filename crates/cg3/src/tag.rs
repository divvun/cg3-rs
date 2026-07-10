//! Port of `src/Tag.hpp` — the `Tag` type and its tag-flag constants, comparison
//! functors, and tag-container typedefs. Wave 2 TYPE-SKELETON pass: only the
//! type definitions are ported here; the method/function bodies
//! (`parseTagRaw`, `rehash`, `toUString`, `parseNumeric`, the copy constructor,
//! and the functor `operator()`s) land in a later pass.
//!
//! Pointer→arena mapping: C++ `Tag*` → [`TagId`], `Set*` → [`SetId`].

use crate::arena::{SetId, TagId};
use crate::flat_unordered_map::FlatUnorderedMap;
use crate::grammar::Grammar;
use crate::inlines::{NUMERIC_MAX, NUMERIC_MIN, hash_value, hash_value_ustring, is_textual};
use crate::math_parser::MathParser;
use crate::sorted_vector::sorted_vector;
use crate::types::{UString, UStringVector};
use regex::Regex;

// C++ `using SetVector = std::vector<Set*>;` (forward-declared in Tag.hpp).
// NOTE(lead): Set.hpp re-declares the identical `SetVector` typedef as
// `set.cg3.set-vector`; in this two-module split the same alias lives in both
// `tag` and `set`. They are byte-identical (`Vec<SetId>`); a later pass may
// canonicalise to a single `crate::set::SetVector`.
/// C++ `using SetVector = std::vector<Set*>;`
pub type SetVector = Vec<SetId>;

// [spec:cg3:def:tag.cg3.c-ops]
/// C++ `enum C_OPS : uint8_t` — the numeric-comparison operators.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
#[repr(u8)]
pub enum C_OPS {
    OP_NOP,
    OP_EQUALS,
    OP_LESSTHAN,
    OP_GREATERTHAN,
    OP_LESSEQUALS,
    OP_GREATEREQUALS,
    OP_NOTEQUALS,
    NUM_OPS,
}

impl Default for C_OPS {
    /// Mirrors the C++ member initialiser `C_OPS comparison_op = OP_NOP;`.
    fn default() -> Self {
        C_OPS::OP_NOP
    }
}

// C++ anonymous `enum : uint32_t` of `Tag::type` bit flags. No spec:def id; the
// bits are reproduced verbatim as `u32` constants.
pub const T_ANY: u32 = 1 << 0;
pub const T_NUMERICAL: u32 = 1 << 1;
pub const T_MAPPING: u32 = 1 << 2;
pub const T_VARIABLE: u32 = 1 << 3;
pub const T_META: u32 = 1 << 4;
pub const T_WORDFORM: u32 = 1 << 5;
pub const T_BASEFORM: u32 = 1 << 6;
pub const T_TEXTUAL: u32 = 1 << 7;
pub const T_DEPENDENCY: u32 = 1 << 8;
pub const T_SAME_BASIC: u32 = 1 << 9;
pub const T_FAILFAST: u32 = 1 << 10;
pub const T_CASE_INSENSITIVE: u32 = 1 << 11;
pub const T_REGEXP: u32 = 1 << 12;
pub const T_PAR_LEFT: u32 = 1 << 13;
pub const T_PAR_RIGHT: u32 = 1 << 14;
pub const T_REGEXP_ANY: u32 = 1 << 15;
pub const T_VARSTRING: u32 = 1 << 16;
pub const T_TARGET: u32 = 1 << 17;
pub const T_MARK: u32 = 1 << 18;
pub const T_ATTACHTO: u32 = 1 << 19;
pub const T_SPECIAL: u32 = 1 << 20;
pub const T_USED: u32 = 1 << 21;
pub const T_LOCAL_VARIABLE: u32 = 1 << 22;
pub const T_SET: u32 = 1 << 23;
pub const T_VSTR: u32 = 1 << 24;
pub const T_ENCL: u32 = 1 << 25;
pub const T_RELATION: u32 = 1 << 26;
pub const T_CONTEXT: u32 = 1 << 27;
pub const T_NUMERIC_MATH: u32 = 1 << 28;
pub const T_PRESERVE_ESC: u32 = 1 << 29;

/// `T_REGEXP_LINE` — ToDo (per C++): remove for real ordered mode.
pub const T_REGEXP_LINE: u32 = 1u32 << 31;

pub const MASK_TAG_SPECIAL: u32 = T_ANY
    | T_TARGET
    | T_MARK
    | T_ATTACHTO
    | T_PAR_LEFT
    | T_PAR_RIGHT
    | T_NUMERICAL
    | T_VARIABLE
    | T_LOCAL_VARIABLE
    | T_META
    | T_FAILFAST
    | T_CASE_INSENSITIVE
    | T_REGEXP
    | T_REGEXP_ANY
    | T_VARSTRING
    | T_SET
    | T_ENCL
    | T_SAME_BASIC
    | T_CONTEXT;

// [spec:cg3:def:tag.cg3.tag]
/// C++ `class Tag`. A single grammar tag: its type-flag mask, cached hashes,
/// text, numeric-comparison payload, varstring pieces, and optional compiled
/// regex.
///
/// NOTE(lead): the C++ `static std::ostream* dump_hashes_out;` (a shared debug
/// sink used by `rehash`) is a class static, not a per-instance field, and is
/// not reproduced here; it is a method-pass / global-I/O concern.
#[derive(Default, Debug)]
pub struct Tag {
    /// `C_OPS comparison_op = OP_NOP;`
    pub comparison_op: C_OPS,
    /// `double comparison_val = 0;`
    pub comparison_val: f64,
    /// `uint32_t type = 0;` (`type` is a Rust keyword → raw identifier).
    pub r#type: u32,
    /// `uint32_t comparison_hash = 0;`
    pub comparison_hash: u32,
    /// `uint32_t dep_self = 0;`
    pub dep_self: u32,
    /// C++ anonymous `union { uint32_t dep_parent = 0; uint32_t variable_hash;
    /// uint32_t context_ref_pos; uint32_t comparison_offset; };`.
    ///
    /// NOTE(lead): every union member is the same `uint32_t` overlaying the
    /// same 4 bytes (a naming alias, not a variant), so the faithful port is a
    /// single `u32`. Reads/writes under the names `variable_hash`,
    /// `context_ref_pos`, and `comparison_offset` in the method pass all target
    /// this field.
    pub dep_parent: u32,
    /// `uint32_t hash = 0;`
    pub hash: u32,
    /// `uint32_t plain_hash = 0;`
    pub plain_hash: u32,
    /// `uint32_t number = 0;`
    pub number: u32,
    /// `uint32_t seed = 0;`
    pub seed: u32,
    /// `UString tag;`
    pub tag: UString,
    /// `UString tag_raw;`
    pub tag_raw: UString,
    /// `std::unique_ptr<SetVector> vs_sets;` — nullable, lazily allocated.
    pub vs_sets: Option<SetVector>,
    /// `std::unique_ptr<UStringVector> vs_names;` — nullable, lazily allocated.
    pub vs_names: Option<UStringVector>,
    /// `mutable URegularExpression* regexp = nullptr;`
    ///
    /// FIELD-TYPE CHANGE (method pass): the Wave-2 `URegularExpression`
    /// placeholder is replaced by `Option<regex::Regex>` — the ICU
    /// `URegularExpression*` owning handle becomes an owned compiled `Regex`.
    /// The pattern is compiled by the grammar/binary parser layer (C++
    /// `parseTag` / `BinaryGrammar_read`), NOT by `parseTagRaw`; here the field
    /// is only *consumed* (the `grammar.regex_tags` scan in `parse_tag_raw`
    /// matches each compiled regex against the tag text via unanchored
    /// `Regex::is_match`, reproducing `uregex_find`). The copy ctor
    /// (`impl Clone`) clones it (C++ `uregex_clone`).
    pub regexp: Option<Regex>,
}

// [spec:cg3:def:tag.cg3.compare-tag]
/// C++ `struct compare_Tag` — strict-weak ordering functor over `Tag*` by
/// `hash`. The `operator()` body is a later (method) pass.
#[derive(Default, Clone, Copy, Debug)]
pub struct compare_Tag;

// [spec:cg3:def:tag.cg3.equal-tag]
/// C++ `struct equal_Tag` — equality functor over `Tag*` by `hash`. The
/// `operator()` body is a later (method) pass.
#[derive(Default, Clone, Copy, Debug)]
pub struct equal_Tag;

// [spec:cg3:def:tag.cg3.compare-tag-vector]
/// C++ `struct compare_TagVector` — lexicographic strict-weak ordering over
/// `TagVector`s by element `hash`. The `operator()` body is a later (method)
/// pass.
#[derive(Default, Clone, Copy, Debug)]
pub struct compare_TagVector;

/// C++ `using TagVector = std::vector<Tag*>;`
pub type TagVector = Vec<TagId>;

/// C++ `using TagList = TagVector;`
pub type TagList = TagVector;

// C++ `using Taguint32HashMap = flat_unordered_map<uint32_t, Tag*>;`.
/// C++ `flat_unordered_map<uint32_t, Tag*>` keyed by tag hash.
pub type Taguint32HashMap = FlatUnorderedMap<u32, TagId>;

// C++ `using TagSortedVector = sorted_vector<Tag*, compare_Tag>;`.
// NOTE(lead): `compare_Tag` orders by `Tag::hash`, which a bare `TagId` cannot
// resolve without the tag arena. The `Comparator<TagId> for compare_Tag` impl
// (the `operator()`) is deferred; when it lands it will need arena access, so
// either this container carries the hash alongside the id or sorting is done
// with an arena-aware comparator externally.
/// C++ `sorted_vector<Tag*, compare_Tag>`.
pub type TagSortedVector = sorted_vector<TagId, compare_Tag>;

// C++ `using TagVectorSet = std::set<TagVector, compare_TagVector>;`.
// NOTE(lead): literal `std::set` → `BTreeSet`. `BTreeSet` orders by `Ord`,
// whereas C++ orders by `compare_TagVector` (element `hash`es, arena-aware).
// The comparator wiring must be reconciled in a later pass (e.g. an
// arena-aware ordered wrapper), as `Vec<TagId>`'s derived `Ord` differs from
// hash order.
/// C++ `std::set<TagVector, compare_TagVector>`.
pub type TagVectorSet = std::collections::BTreeSet<TagVector>;

// ===========================================================================
// Method bodies (Wave 2 method pass). Ported literally from `src/Tag.cpp` /
// `src/Tag.hpp`; each fn carries its `[spec:cg3:def]` + `[spec:cg3:sem]` ids.
// ===========================================================================

impl Tag {
    // [spec:cg3:def:tag.cg3.tag.rehash-fn]
    // [spec:cg3:sem:tag.cg3.tag.rehash-fn]
    /// Recomputes and caches `hash`/`plain_hash` from `type`, `tag`, `seed`.
    ///
    /// HASHING PARITY: the ASCII marker strings (`"^"`, `"META:"`, `"i"`, ...)
    /// are hashed by `hash_value_ustring`, whose UTF-8 bytes equal the C++
    /// `hash_value(const char*)` bytes, so those fold identically. The `tag`
    /// itself is hashed via `hash_value_ustring` over UTF-8 bytes, whereas the
    /// C++ hashed UTF-16 `UChar` code units — so `plain_hash`/`hash` diverge
    /// from the C++ for the tag text (documented deviation of the UTF-8 port;
    /// internally consistent, so hash-dedup still works). The uint32 mixer and
    /// CG3_HASH_SEED remap rules are reproduced exactly by `crate::inlines`.
    /// The static `dump_hashes_out` debug stream is not reproduced.
    pub fn rehash(&mut self) -> u32 {
        self.hash = 0;
        self.plain_hash = 0;

        if self.r#type & T_FAILFAST != 0 {
            self.hash = hash_value_ustring("^", self.hash);
        }

        if self.r#type & T_META != 0 {
            self.hash = hash_value_ustring("META:", self.hash);
        }
        if self.r#type & T_VARIABLE != 0 {
            self.hash = hash_value_ustring("VAR:", self.hash);
        }
        if self.r#type & T_LOCAL_VARIABLE != 0 {
            self.hash = hash_value_ustring("LVAR:", self.hash);
        }
        if self.r#type & T_SET != 0 {
            self.hash = hash_value_ustring("SET:", self.hash);
        }

        self.plain_hash = hash_value_ustring(&self.tag, 0);
        if self.hash != 0 {
            self.hash = hash_value(self.plain_hash, self.hash);
        } else {
            self.hash = self.plain_hash;
        }

        if self.r#type & T_CASE_INSENSITIVE != 0 {
            self.hash = hash_value_ustring("i", self.hash);
        }
        if self.r#type & T_REGEXP != 0 {
            self.hash = hash_value_ustring("r", self.hash);
        }
        if self.r#type & T_VARSTRING != 0 {
            self.hash = hash_value_ustring("v", self.hash);
        }

        self.hash = self.hash.wrapping_add(self.seed);

        self.r#type &= !T_SPECIAL;
        if self.r#type & MASK_TAG_SPECIAL != 0 {
            self.r#type |= T_SPECIAL;
        }

        // `dump_hashes_out` (class-static debug stream) is not ported.

        self.hash
    }

    // [spec:cg3:def:tag.cg3.tag.mark-used-fn]
    // [spec:cg3:sem:tag.cg3.tag.mark-used-fn]
    pub fn mark_used(&mut self) {
        self.r#type |= T_USED;
    }

    // [spec:cg3:def:tag.cg3.tag.allocate-vs-sets-fn]
    // [spec:cg3:sem:tag.cg3.tag.allocate-vs-sets-fn]
    pub fn allocate_vs_sets(&mut self) {
        if self.vs_sets.is_none() {
            self.vs_sets = Some(SetVector::new());
        }
    }

    // [spec:cg3:def:tag.cg3.tag.allocate-vs-names-fn]
    // [spec:cg3:sem:tag.cg3.tag.allocate-vs-names-fn]
    pub fn allocate_vs_names(&mut self) {
        if self.vs_names.is_none() {
            self.vs_names = Some(UStringVector::new());
        }
    }

    // [spec:cg3:def:tag.cg3.tag.parse-numeric-fn]
    // [spec:cg3:sem:tag.cg3.tag.parse-numeric-fn]
    /// Ported over `char`s (the UTF-8 analog of the UChar buffers): `tag.size()`
    /// == the tag's `char` count and the 256-slot stack buffers become bounds
    /// checks. `u_sscanf("%*[<]%[^<>=:!]%[<>=:!]")`, `u_strspn`, the `MAX`/`MIN`
    /// keywords, `%lf`, and the `find_first_of` sets are reproduced inline.
    pub fn parse_numeric(&mut self, trusted: bool) {
        let chars: Vec<char> = self.tag.chars().collect();
        let size = chars.len();
        if size >= 256 {
            return;
        }

        // u_sscanf(tag, "%*[<]%[^<>=:!]%[<>=:!]", &tkey, &top) == 2 && top[0]
        let opset = ['<', '>', '=', ':', '!'];
        let mut i = 0usize;
        // %*[<] : discard a run of '<' (scanset needs >= 1 char, else count 0).
        let mut n_lt = 0usize;
        while i < size && chars[i] == '<' {
            i += 1;
            n_lt += 1;
        }
        if n_lt == 0 {
            return;
        }
        // %[^<>=:!] : tkey (>= 1 char, else the '== 2' check fails).
        let key_start = i;
        while i < size && !opset.contains(&chars[i]) {
            i += 1;
        }
        let tkey: Vec<char> = chars[key_start..i].to_vec();
        if tkey.is_empty() {
            return;
        }
        // %[<>=:!] : top (>= 1 char, else count 1 -> '!= 2' -> return).
        let op_start = i;
        while i < size && opset.contains(&chars[i]) {
            i += 1;
        }
        let top: Vec<char> = chars[op_start..i].to_vec();
        if top.is_empty() {
            // count == 1 (top not matched) OR top[0] == 0.
            return;
        }

        let tkz = tkey.len();
        let toz = top.len();
        if tkz + toz + 1 >= size {
            return;
        }

        // txval = copy(tag[tkz+toz+1 .. size-1]); the C++ NUL-terminates just
        // past it, so an index at/after txval.len() reads as '\0' here.
        let txval: Vec<char> = chars[(tkz + toz + 1)..(size - 1)].to_vec();
        let tget = |k: usize| -> char {
            if k < txval.len() {
                txval[k]
            } else {
                '\0'
            }
        };
        if txval.is_empty() {
            return;
        }

        let mut tval: f64 = 0.0;
        // r = u_strspn(txval, "-.0123456789")
        let numset = [
            '-', '.', '0', '1', '2', '3', '4', '5', '6', '7', '8', '9',
        ];
        let mut r = 0usize;
        while r < txval.len() && numset.contains(&txval[r]) {
            r += 1;
        }

        let txval_str: String = txval.iter().collect();
        // find_first_of sets: math delimiters present, "structural" chars absent.
        let math_set = "-+*/^%()=";
        let neg_set = "\"\\<>[]{}!?&$\u{A4}#\u{A3}@~`\u{B4}';:,|_";
        if trusted
            && tget(r) != '\0'
            && txval_str.chars().any(|c| math_set.contains(c))
            && !txval_str.chars().any(|c| neg_set.contains(c))
        {
            // comparison_offset (union member aliasing dep_parent).
            self.dep_parent = (tkey.len() + top.len() + 1) as u32;
            let comparison_offset = self.dep_parent as usize;
            // exp = view(tag).remove_prefix(comparison_offset).remove_suffix(1)
            let exp_str: String = chars[comparison_offset..(size - 1)].iter().collect();
            let mut mp = MathParser::new(NUMERIC_MIN, NUMERIC_MAX);
            if mp.eval(&exp_str).is_err() {
                self.dep_parent = 0;
                return;
            }
            self.r#type |= T_NUMERIC_MATH;
        } else if tget(0) == 'M' && tget(1) == 'A' && tget(2) == 'X' && tget(3) == '\0' {
            tval = NUMERIC_MAX;
        } else if tget(0) == 'M' && tget(1) == 'I' && tget(2) == 'N' && tget(3) == '\0' {
            tval = NUMERIC_MIN;
        } else if tget(r) != '\0' || !scan_double(&txval, &mut tval) {
            return;
        }

        if tval < NUMERIC_MIN {
            tval = NUMERIC_MIN;
        }
        if tval > NUMERIC_MAX {
            tval = NUMERIC_MAX;
        }

        let top0 = top[0];
        let top1 = top.get(1).copied().unwrap_or('\0');
        if top0 == '<' {
            self.comparison_op = C_OPS::OP_LESSTHAN;
        } else if top0 == '>' {
            self.comparison_op = C_OPS::OP_GREATERTHAN;
        } else if top0 == '=' || top0 == ':' {
            self.comparison_op = C_OPS::OP_EQUALS;
        } else if top0 == '!' {
            self.comparison_op = C_OPS::OP_NOTEQUALS;
        }
        if top1 != '\0' {
            if top1 == '=' || top1 == ':' {
                if self.comparison_op == C_OPS::OP_GREATERTHAN {
                    self.comparison_op = C_OPS::OP_GREATEREQUALS;
                } else if self.comparison_op == C_OPS::OP_LESSTHAN {
                    self.comparison_op = C_OPS::OP_LESSEQUALS;
                } else if self.comparison_op == C_OPS::OP_NOTEQUALS {
                    self.comparison_op = C_OPS::OP_NOTEQUALS;
                }
            } else if top1 == '>' {
                if self.comparison_op == C_OPS::OP_EQUALS {
                    self.comparison_op = C_OPS::OP_GREATEREQUALS;
                } else if self.comparison_op == C_OPS::OP_LESSTHAN {
                    self.comparison_op = C_OPS::OP_NOTEQUALS;
                }
            } else if top1 == '<' {
                if self.comparison_op == C_OPS::OP_EQUALS {
                    self.comparison_op = C_OPS::OP_LESSEQUALS;
                } else if self.comparison_op == C_OPS::OP_GREATERTHAN {
                    self.comparison_op = C_OPS::OP_NOTEQUALS;
                }
            }
        }
        self.comparison_val = tval;
        let tkey_str: String = tkey.iter().collect();
        self.comparison_hash = hash_value_ustring(&tkey_str, 0);
        self.r#type |= T_NUMERICAL;
    }

    // [spec:cg3:def:tag.cg3.tag.to-u-string-fn]
    // [spec:cg3:sem:tag.cg3.tag.to-u-string-fn]
    pub fn to_u_string(&self, escape: bool) -> UString {
        if !self.tag_raw.is_empty() {
            return self.tag_raw.clone();
        }

        let mut str = UString::new();
        str.reserve(self.tag.len());

        if self.r#type & T_FAILFAST != 0 {
            str.push('^');
        }
        if self.r#type & T_META != 0 {
            str.push_str("META:");
        }
        if self.r#type & T_VARIABLE != 0 {
            str.push_str("VAR:");
        }
        if self.r#type & T_LOCAL_VARIABLE != 0 {
            str.push_str("LVAR:");
        }
        if self.r#type & T_SET != 0 {
            str.push_str("SET:");
        }
        if self.r#type & T_VSTR != 0 {
            str.push_str("VSTR:");
        }

        if self.r#type & (T_CASE_INSENSITIVE | T_REGEXP) != 0 && !is_textual(&self.tag) {
            str.push('/');
        }

        // C++ `tag[0]` on an empty string yields the '\0' terminator.
        let first = self.tag.chars().next().unwrap_or('\0');
        if escape && first != '"' {
            for c in self.tag.chars() {
                if c == '\\' || c == '(' || c == ')' || c == ';' || c == '#' || c == ' ' {
                    str.push('\\');
                }
                str.push(c);
            }
        } else {
            str.push_str(&self.tag);
        }

        if self.r#type & (T_CASE_INSENSITIVE | T_REGEXP) != 0 && !is_textual(&self.tag) {
            str.push('/');
        }
        if self.r#type & T_REGEXP_LINE != 0 {
            str.push('l');
        } else if self.r#type & (T_REGEXP | T_REGEXP_ANY) != 0 {
            str.push('r');
        }
        if self.r#type & T_CASE_INSENSITIVE != 0 {
            str.push('i');
        }
        if (self.r#type & T_VARSTRING != 0) && (self.r#type & T_VSTR == 0) {
            str.push('v');
        }
        str
    }
}

// [spec:cg3:def:tag.cg3.tag.tag-fn]
// [spec:cg3:sem:tag.cg3.tag.tag-fn]
/// Copy constructor `Tag(const Tag& o)`. In the port every C++ `Tag` copy goes
/// through this ctor, so it is the faithful mapping of `Clone`. QUIRKS
/// reproduced verbatim: `tag_raw` is NOT copied (left default-empty) and
/// `regexp` is cloned (C++ `uregex_clone`, ignoring the ICU status) rather than
/// left null in the init list.
impl Clone for Tag {
    fn clone(&self) -> Self {
        let o = self;
        let mut t = Tag {
            comparison_op: o.comparison_op,
            comparison_val: o.comparison_val,
            r#type: o.r#type,
            comparison_hash: o.comparison_hash,
            dep_self: o.dep_self,
            dep_parent: o.dep_parent,
            hash: o.hash,
            plain_hash: o.plain_hash,
            number: o.number,
            seed: o.seed,
            tag: o.tag.clone(),
            // QUIRK: `tag_raw` is NOT copied by the C++ ctor.
            tag_raw: UString::new(),
            vs_sets: None,
            vs_names: None,
            regexp: None,
        };

        if let Some(names) = &o.vs_names {
            t.allocate_vs_names();
            *t.vs_names.as_mut().unwrap() = names.clone();
        }
        if let Some(sets) = &o.vs_sets {
            t.allocate_vs_sets();
            *t.vs_sets.as_mut().unwrap() = sets.clone();
        }
        if let Some(re) = &o.regexp {
            // uregex_clone(o.regexp, &status) — status ignored/unused.
            t.regexp = Some(re.clone());
        }
        t
    }
}

// [spec:cg3:def:tag.cg3.tag.parse-tag-raw-fn]
// [spec:cg3:sem:tag.cg3.tag.parse-tag-raw-fn]
/// Free fn (interning/allocation touches `Grammar`): C++
/// `void Tag::parseTagRaw(const UChar* to, Grammar* grammar)`. `this` is a
/// standalone tag (not yet in the grammar arena) — its C++ call sites are
/// `new Tag()` in `allocateTag` and a fresh tag in `GrammarApplicator`, so there
/// is no aliasing with the grammar arena.
///
/// The `grammar->regex_tags` scan (`uregex_setText` + `uregex_find`) becomes an
/// unanchored `Regex::is_match` against the tag text using each regex-tag's
/// compiled `regexp` (anchoring is baked into the pattern at compile time in the
/// parser layer). `grammar->icase_tags` uses `ux_str_case_compare` (ICU
/// `u_strCaseCompare`, approximated with Unicode lowercase folding).
pub fn parse_tag_raw(this: &mut Tag, to: &str, grammar: &mut Grammar) {
    this.r#type = 0;
    let to_chars: Vec<char> = to.chars().collect();
    let length = to_chars.len();
    debug_assert!(length != 0, "parseTagRaw() will not work with empty strings.");

    // NUL-terminator-aware indexed access: index >= length reads as '\0'
    // (matching C++ `std::string::operator[]` at `size()` and the short-circuits
    // that guard reads past it).
    let cat = |k: usize| -> char {
        if k < length {
            to_chars[k]
        } else {
            '\0'
        }
    };

    if length != 0 && (to_chars[0] == '"' || to_chars[0] == '<') {
        if (to_chars[0] == '"' && cat(length - 1) == '"')
            || (to_chars[0] == '<' && cat(length - 1) == '>')
        {
            this.r#type |= T_TEXTUAL;
            if to_chars[0] == '"' && cat(length - 1) == '"' {
                if cat(1) == '<' && cat(length - 2) == '>' && length > 4 {
                    this.r#type |= T_WORDFORM;
                } else {
                    this.r#type |= T_BASEFORM;
                }
            }
        }
    }

    // tag.assign(to, length)
    this.tag = to.to_string();

    // grammar->regex_tags scan: uregex_setText + uregex_find == unanchored
    // is_match against the tag text. Collect ids first to end the borrows.
    let regex_ids: Vec<TagId> = grammar.regex_tags.iter().copied().collect();
    for tid in regex_ids {
        if let Some(re) = &grammar.single_tags_list[tid.0].regexp
            && re.is_match(&this.tag)
        {
            this.r#type |= T_TEXTUAL;
        }
    }
    // grammar->icase_tags scan.
    let icase_ids: Vec<TagId> = grammar.icase_tags.iter().copied().collect();
    for tid in icase_ids {
        if ux_str_case_compare(&this.tag, &grammar.single_tags_list[tid.0].tag) {
            this.r#type |= T_TEXTUAL;
        }
    }

    if cat(0) == '<' && cat(length - 1) == '>' {
        this.parse_numeric(false);
    }
    if cat(0) == '#' {
        // u_sscanf("#%i->%i", &dep_self, &dep_parent) == 2 && dep_self != 0
        let (n, v1, v2) = scan_hash_i_arrow_i(&to_chars, &['-', '>']);
        if let Some(v) = v1 {
            this.dep_self = v;
        }
        if let Some(v) = v2 {
            this.dep_parent = v;
        }
        if n == 2 && this.dep_self != 0 {
            this.r#type |= T_DEPENDENCY;
        }
        // Unicode-arrow form: u_sscanf_u("#%i\u{2192}%i", ...)
        let (n, v1, v2) = scan_hash_i_arrow_i(&to_chars, &['\u{2192}']);
        if let Some(v) = v1 {
            this.dep_self = v;
        }
        if let Some(v) = v2 {
            this.dep_parent = v;
        }
        if n == 2 && this.dep_self != 0 {
            this.r#type |= T_DEPENDENCY;
        }
    }
    if cat(0) == 'I' && cat(1) == 'D' && cat(2) == ':' && u_isdigit(cat(3)) {
        // u_sscanf("ID:%i", &dep_self) == 1 && dep_self != 0
        if let Some(v) = scan_id(&to_chars) {
            this.dep_self = v;
            if this.dep_self != 0 {
                this.r#type |= T_RELATION;
            }
        }
    }
    if cat(0) == 'R' && cat(1) == ':' {
        // dep_parent = UINT32_MAX; u_sscanf("R:%[^:]:%i", &relname, &dep_parent)
        this.dep_parent = u32::MAX;
        let (n, relname, dp) = scan_relation(&to_chars);
        if let Some(v) = dp {
            this.dep_parent = v;
        }
        if n == 2 && this.dep_parent != u32::MAX {
            this.r#type |= T_RELATION;
            let reltag = allocate_tag(grammar, &relname);
            this.comparison_hash = grammar.single_tags_list[reltag.0].hash;
        }
    }

    this.r#type &= !T_SPECIAL;
    if this.r#type & T_NUMERICAL != 0 {
        this.r#type |= T_SPECIAL;
    }
}

// ---------------------------------------------------------------------------
// Comparators. COMPARATOR-SIGNATURE CONVENTION: `compare_Tag`/`equal_Tag`/
// `compare_TagVector` order by `Tag::hash`, but the containers hold `TagId`s
// which cannot resolve a hash without the tag arena. Per the task's arena
// model, they are ported as FREE FNS taking `grammar: &Grammar` + the ids (the
// C++ dereferenced the `Tag*`; here the arena provides the same value). The
// marker structs above are kept for the `TagSortedVector`/`TagVectorSet` type
// aliases; a later pass wires an arena-aware ordered container.
// ---------------------------------------------------------------------------

// [spec:cg3:def:tag.cg3.compare-tag.operator-fn]
// [spec:cg3:sem:tag.cg3.compare-tag.operator-fn]
pub fn compare_tag(grammar: &Grammar, a: TagId, b: TagId) -> bool {
    grammar.single_tags_list[a.0].hash < grammar.single_tags_list[b.0].hash
}

// [spec:cg3:def:tag.cg3.equal-tag.operator-fn]
// [spec:cg3:sem:tag.cg3.equal-tag.operator-fn]
pub fn equal_tag(grammar: &Grammar, a: TagId, b: TagId) -> bool {
    grammar.single_tags_list[a.0].hash == grammar.single_tags_list[b.0].hash
}

// [spec:cg3:def:tag.cg3.compare-tag-vector.operator-fn]
// [spec:cg3:sem:tag.cg3.compare-tag-vector.operator-fn]
pub fn compare_tag_vector(grammar: &Grammar, a: &TagVector, b: &TagVector) -> bool {
    let mut i = 0usize;
    while i < a.len() && i < b.len() {
        let ha = grammar.single_tags_list[a[i].0].hash;
        let hb = grammar.single_tags_list[b[i].0].hash;
        if ha != hb {
            return ha < hb;
        }
        i += 1;
    }
    a.len() < b.len()
}

// [spec:cg3:def:tag.cg3.fill-tagvector-fn]
// [spec:cg3:sem:tag.cg3.fill-tagvector-fn]
/// Template helper `fill_tagvector(const T& in, ...)`; the generic input becomes
/// a `&[TagId]` and `tag->type` is resolved via the tag arena. `did`/`special`
/// are only ever set to `true` (accumulating; the caller pre-initializes them).
pub fn fill_tagvector(
    grammar: &Grammar,
    in_: &[TagId],
    tags: &mut TagVector,
    did: &mut bool,
    special: &mut bool,
) {
    for &tag in in_ {
        let ty = grammar.single_tags_list[tag.0].r#type;
        if ty & T_NUMERICAL != 0 {
            *did = true;
        } else {
            if ty & T_SPECIAL != 0 {
                *special = true;
            }
            tags.push(tag);
        }
    }
}

// ---------------------------------------------------------------------------
// Grammar interning needed by `parse_tag_raw`'s `R:` branch. These reproduce
// `Grammar::allocateTag(const UChar*)` + `Grammar::addTag` faithfully enough for
// the relation-tag hash, using only the public `Grammar` fields and the ported
// `Tag` methods. Placed here (not annotated with the grammar spec ids, which
// belong to `grammar.rs`) so `parse_tag_raw` compiles with only `tag.rs` edited.
//
// Registration into `single_tags` mirrors `Grammar::addTag`'s
// `single_tags[hash + seed] = tag` so relation-name tags resolve later (e.g.
// printReading's `grammar->single_tags.find(comparison_hash)` for `R:name:n`
// output; Wave-3 T_MergeCohorts).
fn allocate_tag(grammar: &mut Grammar, txt: &[char]) -> TagId {
    let txt_str: String = txt.iter().collect();
    // txt[0] == 0 / '(' are CG3Quit diagnostics in C++ (parser I/O); omitted.
    let thash = hash_value_ustring(&txt_str, 0);
    let found: Option<TagId> = {
        let it = grammar.single_tags.find(thash);
        if it != grammar.single_tags.end() {
            Some(it.get().1)
        } else {
            None
        }
    };
    if let Some(tid) = found {
        let existing = &grammar.single_tags_list[tid.0];
        if !existing.tag.is_empty() && existing.tag == txt_str {
            return tid;
        }
    }

    let mut tag = Tag::default();
    parse_tag_raw(&mut tag, &txt_str, grammar);
    add_tag(grammar, tag)
}

fn add_tag(grammar: &mut Grammar, mut tag: Tag) -> TagId {
    let hash = tag.rehash();
    // Seed probe, faithful to Grammar::addTag.
    let mut existing: Option<TagId> = None;
    let mut chosen_seed: Option<u32> = None;
    let mut seed = 0u32;
    while seed < 10000 {
        let ih = hash.wrapping_add(seed);
        let found: Option<TagId> = {
            let it = grammar.single_tags.find(ih);
            if it != grammar.single_tags.end() {
                Some(it.get().1)
            } else {
                None
            }
        };
        match found {
            Some(t_id) => {
                // C++ `t == tag` (identity) never holds for a fresh, un-interned
                // tag, so only the text-equality dedup applies.
                if grammar.single_tags_list[t_id.0].tag == tag.tag {
                    // C++ `hash += seed; return single_tags[hash]` — subsumed by
                    // returning `t_id` directly (== `single_tags[hash + seed]`).
                    existing = Some(t_id);
                    break;
                }
            }
            None => {
                chosen_seed = Some(seed);
                break;
            }
        }
        seed += 1;
    }

    if let Some(t_id) = existing {
        return t_id;
    }

    let seed = chosen_seed.expect("addTag: seed space exhausted");
    tag.seed = seed;
    let _hash = tag.rehash();
    let idx = grammar.single_tags_list.alloc(tag);
    // tag->number = single_tags_list.size() - 1 (== idx when appending, as the
    // parse phase never frees arena slots).
    grammar.single_tags_list[idx].number = idx;
    grammar.single_tags.insert((_hash, crate::arena::TagId(idx)));
    TagId(idx)
}

// ---------------------------------------------------------------------------
// Local stand-ins for ICU helpers used above (they belong to other, not-yet-
// wired modules — `uextras`/`u_sscanf`/`u_isdigit`). Deliberately un-annotated;
// reimplemented here so this file compiles standalone (cf. `math_parser.rs`).
// ---------------------------------------------------------------------------

/// `uextras.hpp` `ux_strCaseCompare(a, b)`: ICU `u_strCaseCompare` with
/// `U_FOLD_CASE_DEFAULT`, true on full case-fold equality. Approximated with
/// Unicode simple lowercase folding (ICU-vs-Rust parity risk for non-ASCII).
fn ux_str_case_compare(a: &str, b: &str) -> bool {
    a.chars()
        .flat_map(char::to_lowercase)
        .eq(b.chars().flat_map(char::to_lowercase))
}

/// ICU `u_isdigit` (decimal-digit category), approximated with Rust's Unicode
/// numeric table.
fn u_isdigit(c: char) -> bool {
    c.is_numeric()
}

/// `u_sscanf(txval, "%lf", &tval)`: parses a leading `strtod`-style double and
/// writes it to `out`, returning whether a number was read (== the C `1` count).
fn scan_double(chars: &[char], out: &mut f64) -> bool {
    let mut i = 0usize;
    while i < chars.len() && chars[i].is_whitespace() {
        i += 1;
    }
    let start = i;
    if i < chars.len() && (chars[i] == '+' || chars[i] == '-') {
        i += 1;
    }
    let mut has_digits = false;
    while i < chars.len() && chars[i].is_ascii_digit() {
        i += 1;
        has_digits = true;
    }
    if i < chars.len() && chars[i] == '.' {
        i += 1;
        while i < chars.len() && chars[i].is_ascii_digit() {
            i += 1;
            has_digits = true;
        }
    }
    if !has_digits {
        return false;
    }
    if i < chars.len() && (chars[i] == 'e' || chars[i] == 'E') {
        let mut j = i + 1;
        if j < chars.len() && (chars[j] == '+' || chars[j] == '-') {
            j += 1;
        }
        if j < chars.len() && chars[j].is_ascii_digit() {
            while j < chars.len() && chars[j].is_ascii_digit() {
                j += 1;
            }
            i = j;
        }
    }
    let s: String = chars[start..i].iter().collect();
    match s.parse::<f64>() {
        Ok(v) => {
            *out = v;
            true
        }
        Err(_) => false,
    }
}

/// C `%i` integer conversion (auto-base: `0x`->hex, leading `0`->octal, else
/// decimal) at `pos`. Returns `(value_as_u32, next_pos)`, or `None` when no
/// digit was read. The value is truncated/wrapped into `u32` like the C++
/// `uint32_t*` target.
fn scan_i(chars: &[char], mut pos: usize) -> Option<(u32, usize)> {
    while pos < chars.len() && chars[pos].is_whitespace() {
        pos += 1;
    }
    let mut neg = false;
    if pos < chars.len() && (chars[pos] == '+' || chars[pos] == '-') {
        neg = chars[pos] == '-';
        pos += 1;
    }
    let mut base = 10u32;
    if pos < chars.len() && chars[pos] == '0' {
        if pos + 1 < chars.len() && (chars[pos + 1] == 'x' || chars[pos + 1] == 'X') {
            base = 16;
            pos += 2;
        } else {
            base = 8; // '0' is retained as an octal digit below.
        }
    }
    let mut val: u64 = 0;
    let mut any = false;
    while pos < chars.len() {
        match chars[pos].to_digit(base) {
            Some(d) => {
                val = val.wrapping_mul(base as u64).wrapping_add(d as u64);
                pos += 1;
                any = true;
            }
            None => break,
        }
    }
    if !any {
        return None;
    }
    let mut out = val as u32;
    if neg {
        out = out.wrapping_neg();
    }
    Some((out, pos))
}

/// `u_sscanf("#%i<arrow>%i", &a, &b)`: returns `(count, a?, b?)` where `count`
/// is the number of assigned `%i` conversions (0..=2).
fn scan_hash_i_arrow_i(chars: &[char], arrow: &[char]) -> (u32, Option<u32>, Option<u32>) {
    if chars.first() != Some(&'#') {
        return (0, None, None);
    }
    let (v1, mut pos) = match scan_i(chars, 1) {
        Some(x) => x,
        None => return (0, None, None),
    };
    for &ac in arrow {
        if pos >= chars.len() || chars[pos] != ac {
            return (1, Some(v1), None);
        }
        pos += 1;
    }
    match scan_i(chars, pos) {
        Some((v2, _)) => (2, Some(v1), Some(v2)),
        None => (1, Some(v1), None),
    }
}

/// `u_sscanf("ID:%i", &dep_self)`: `Some(dep_self)` iff exactly one conversion.
fn scan_id(chars: &[char]) -> Option<u32> {
    if chars.len() >= 3 && chars[0] == 'I' && chars[1] == 'D' && chars[2] == ':' {
        scan_i(chars, 3).map(|(v, _)| v)
    } else {
        None
    }
}

/// `u_sscanf("R:%[^:]:%i", &relname, &dep_parent)`: returns
/// `(count, relname, dep_parent?)`; `count` is the number of assigned
/// conversions (`%[^:]` then `%i`).
fn scan_relation(chars: &[char]) -> (u32, Vec<char>, Option<u32>) {
    if !(chars.len() >= 2 && chars[0] == 'R' && chars[1] == ':') {
        return (0, Vec::new(), None);
    }
    let start = 2usize;
    let mut pos = start;
    while pos < chars.len() && chars[pos] != ':' {
        pos += 1;
    }
    let relname: Vec<char> = chars[start..pos].to_vec();
    if relname.is_empty() {
        return (0, Vec::new(), None);
    }
    // literal ':'
    if pos >= chars.len() || chars[pos] != ':' {
        return (1, relname, None);
    }
    pos += 1;
    match scan_i(chars, pos) {
        Some((v, _)) => (2, relname, Some(v)),
        None => (1, relname, None),
    }
}
