//! `inlines.hpp` — misc utilities, RAII guards, reversed-range iteration, erase, concat.
//!
//! Split out of the wave-2 monolithic `inlines.rs` (wave 4, w4-file-split-fmt).

#![allow(non_camel_case_types)]
#![allow(dead_code)]

use crate::types::{UString, UStringView};

// ---------------------------------------------------------------------------
// Misc utilities
// ---------------------------------------------------------------------------

// [spec:cg3:def:inlines.cg3.cg3-quit-fn]
// [spec:cg3:sem:inlines.cg3.cg3-quit-fn]
// [[noreturn]] -> `-> !`. Prints the diagnostic to stderr iff file is Some and
// line != 0, then terminates. Wave 4: the termination is a [`crate::error::Cg3Exit`]
// unwind — only the `src/bin` entry points turn it into a real process exit
// (see `crate::error::run_cli`), so library embedders keep their process.
pub fn cg3_quit(c: i32, file: Option<&str>, line: u32) -> ! {
    if let Some(file) = file
        && line != 0
    {
        tracing::error!("CG3Quit triggered from {} line {}.", file, line);
    }
    crate::error::cg3_exit(c)
}

// [spec:cg3:def:inlines.cg3.usv-fn]
// [spec:cg3:sem:inlines.cg3.usv-fn]
// Only the `USV(UString&)` overload is ported (it simply returns a view); the
// `USV(UnicodeString&)` overload requires ICU and is omitted in Wave 2.
#[inline]
pub fn usv(str: &UString) -> UStringView<'_> {
    str
}

// [spec:cg3:def:inlines.cg3.size-fn]
// [spec:cg3:sem:inlines.cg3.size-fn]
#[inline]
pub const fn size<T, const N: usize>(_: &[T; N]) -> usize {
    N
}

// A container that can report empty-ness and clear itself (stand-in for the C++
// template's implicit `.empty()`/`.clear()` requirement). Other modules may
// impl this for their own container types.
pub trait Clearable {
    fn is_empty_c(&self) -> bool;
    fn clear_c(&mut self);
}

impl<T> Clearable for Vec<T> {
    #[inline]
    fn is_empty_c(&self) -> bool {
        self.is_empty()
    }
    #[inline]
    fn clear_c(&mut self) {
        self.clear();
    }
}

impl Clearable for String {
    #[inline]
    fn is_empty_c(&self) -> bool {
        self.is_empty()
    }
    #[inline]
    fn clear_c(&mut self) {
        self.clear();
    }
}

// [spec:cg3:def:inlines.cg3.clear-fn]
// [spec:cg3:sem:inlines.cg3.clear-fn]
#[inline]
pub fn clear<C: Clearable>(c: &mut C) {
    if !c.is_empty_c() {
        c.clear_c();
    }
}

// [spec:cg3:def:inlines.cg3.is-textual-fn]
// [spec:cg3:sem:inlines.cg3.is-textual-fn]
// Ported over bytes (AsRef<[u8]>): the delimiters compared are all ASCII, so a
// byte-level front/back check is faithful. Panics on empty `s` (C++ front()/
// back() on empty is UB). PARITY: if the last char is multibyte and non-ASCII,
// the last byte != '"'/'>' — same result as the C++ (last code unit != '"').
#[inline]
pub fn is_textual<S: AsRef<[u8]>>(s: S) -> bool {
    let s = s.as_ref();
    let front = s[0];
    let back = s[s.len() - 1];
    (front == b'"' && back == b'"') || (front == b'<' && back == b'>')
}

// [spec:cg3:def:inlines.cg3.is-internal-fn]
// [spec:cg3:sem:inlines.cg3.is-internal-fn]
#[inline]
pub fn is_internal<S: AsRef<[u8]>>(s: S) -> bool {
    let s = s.as_ref();
    s[0] == b'_' && s[1] == b'G' && s[2] == b'_'
}

// [spec:cg3:def:inlines.cg3.is-cg3b-fn]
// [spec:cg3:sem:inlines.cg3.is-cg3b-fn]
#[inline]
pub fn is_cg3b<S: AsRef<[u8]>>(s: S) -> bool {
    let s = s.as_ref();
    s[0] == b'C' && s[1] == b'G' && s[2] == b'3' && s[3] == b'B'
}

// [spec:cg3:def:inlines.cg3.is-cg3bsf-fn]
// [spec:cg3:sem:inlines.cg3.is-cg3bsf-fn]
#[inline]
pub fn is_cg3bsf<S: AsRef<[u8]>>(s: S) -> bool {
    let s = s.as_ref();
    s[0] == b'C' && s[1] == b'G' && s[2] == b'B' && s[3] == b'F'
}

// [spec:cg3:def:inlines.cg3.insert-if-exists-fn]
// [spec:cg3:sem:inlines.cg3.insert-if-exists-fn]
// boost::dynamic_bitset -> Vec<bool> stand-in (bit i == vec[i]); no bitset type
// nor boost/external crate exists in Wave 2. Grows `cont` (zero-fill) then ORs.
pub fn insert_if_exists(cont: &mut Vec<bool>, other: Option<&Vec<bool>>) {
    if let Some(other) = other
        && !other.is_empty()
    {
        let newlen = cont.len().max(other.len());
        cont.resize(newlen, false);
        for (i, &bit) in other.iter().enumerate() {
            if bit {
                cont[i] = true;
            }
        }
    }
}

// [spec:cg3:def:inlines.cg3.g-app-set-opts-ranged-fn]
// [spec:cg3:sem:inlines.cg3.g-app-set-opts-ranged-fn]
// Parses comma-separated numbers/inclusive ranges. `value` (C++ `const char*`)
// -> `&str`; scanning is over its bytes with hand-ported atoi/strchr. `cont` is
// a Vec<u32>. Inclusive ranges use Rust's `low..=high` (empty when high < low,
// matching the uint32 `low <= high` false case for e.g. "3-1").
pub fn g_app_set_opts_ranged(value: &str, cont: &mut Vec<u32>, fill: bool) {
    let vb = value.as_bytes();
    cont.clear();
    let mut had_range = false;

    let mut comma = 0usize;
    loop {
        let low = atoi(vb, comma).unsigned_abs();
        let mut high = low;
        let delim = strchr(vb, comma, b'-');
        let nextc = strchr(vb, comma, b',');
        if let Some(d) = delim
            && (nextc.is_none() || nextc.unwrap() > d)
        {
            had_range = true;
            high = atoi(vb, d + 1).unsigned_abs();
        }
        for v in low..=high {
            cont.push(v);
        }

        // do-while: (comma = strchr(comma,',')) != 0 && ++comma && *comma != 0
        let c = match strchr(vb, comma, b',') {
            Some(c) => c,
            None => break,
        };
        comma = c + 1;
        if !(comma < vb.len() && vb[comma] != 0) {
            break;
        }
    }

    if cont.len() == 1 && !had_range && fill {
        let val = cont[0];
        cont.clear();
        for i in 1..=val {
            cont.push(i);
        }
    }
}

// C `atoi` starting at `start`: skip leading whitespace, optional sign, digits.
fn atoi(s: &[u8], mut i: usize) -> i32 {
    while i < s.len() && (s[i] as char).is_ascii_whitespace() {
        i += 1;
    }
    let mut sign: i64 = 1;
    if i < s.len() && (s[i] == b'+' || s[i] == b'-') {
        if s[i] == b'-' {
            sign = -1;
        }
        i += 1;
    }
    let mut n: i64 = 0;
    while i < s.len() && s[i].is_ascii_digit() {
        n = n * 10 + (s[i] - b'0') as i64;
        i += 1;
    }
    (sign * n) as i32
}

// C `strchr(s + start, ch)`: index of first `ch` at/after `start`, else None.
fn strchr(s: &[u8], start: usize, ch: u8) -> Option<usize> {
    s[start..].iter().position(|&b| b == ch).map(|p| start + p)
}

// ---------------------------------------------------------------------------
// RAII guards
// ---------------------------------------------------------------------------

// [spec:cg3:def:inlines.cg3.swapper]
pub struct swapper<'a, T> {
    pub(crate) cond: bool,
    pub(crate) a: &'a mut T,
    pub(crate) b: &'a mut T,
}

impl<'a, T> swapper<'a, T> {
    // [spec:cg3:def:inlines.cg3.swapper.swapper-fn]
    // [spec:cg3:sem:inlines.cg3.swapper.swapper-fn]
    pub fn new(cond: bool, a: &'a mut T, b: &'a mut T) -> swapper<'a, T> {
        if cond {
            std::mem::swap(a, b);
        }
        swapper { cond, a, b }
    }
}

impl<'a, T> Drop for swapper<'a, T> {
    fn drop(&mut self) {
        if self.cond {
            std::mem::swap(&mut *self.a, &mut *self.b);
        }
    }
}

// [spec:cg3:def:inlines.cg3.swapper-false]
// The C++ nests a `swapper<bool>` over an internal `val`, which is
// self-referential (swp borrows val) and cannot be expressed in safe Rust. The
// equivalent NET effect is implemented directly: while alive (cond true), `b`
// is held at false and restored to its original value on drop.
pub struct swapper_false<'a> {
    pub(crate) cond: bool,
    pub(crate) b: &'a mut bool,
    pub(crate) old: bool,
}

impl<'a> swapper_false<'a> {
    // [spec:cg3:def:inlines.cg3.swapper-false.swapper-false-fn]
    // [spec:cg3:sem:inlines.cg3.swapper-false.swapper-false-fn]
    pub fn new(cond: bool, b: &'a mut bool) -> swapper_false<'a> {
        let old = *b;
        if cond {
            *b = false;
        }
        swapper_false { cond, b, old }
    }
}

impl<'a> Drop for swapper_false<'a> {
    fn drop(&mut self) {
        if self.cond {
            *self.b = self.old;
        }
    }
}

// [spec:cg3:def:inlines.cg3.uncond-swap]
pub struct uncond_swap<'a, T> {
    pub(crate) a: &'a mut T,
    pub(crate) b: T,
}

impl<'a, T> uncond_swap<'a, T> {
    // [spec:cg3:def:inlines.cg3.uncond-swap.uncond-swap-fn]
    // [spec:cg3:sem:inlines.cg3.uncond-swap.uncond-swap-fn]
    // `b` is taken by value (a copy/move); after construction `a` holds the
    // passed value and `b_` holds a's original.
    pub fn new(a: &'a mut T, mut b: T) -> uncond_swap<'a, T> {
        std::mem::swap(a, &mut b);
        uncond_swap { a, b }
    }
}

impl<'a, T> Drop for uncond_swap<'a, T> {
    fn drop(&mut self) {
        std::mem::swap(&mut *self.a, &mut self.b);
    }
}

// Provides `++`/`--` for the inc_dec counter guard.
pub trait Incrementable {
    fn increment(&mut self);
    fn decrement(&mut self);
}

macro_rules! impl_incrementable {
    ($($t:ty),*) => {$(
        impl Incrementable for $t {
            #[inline] fn increment(&mut self) { *self += 1; }
            #[inline] fn decrement(&mut self) { *self -= 1; }
        }
    )*};
}
impl_incrementable!(i8, u8, i16, u16, i32, u32, i64, u64, isize, usize);

// [spec:cg3:def:inlines.cg3.inc-dec]
pub struct inc_dec<'a, T: Incrementable> {
    p: Option<&'a mut T>,
}

impl<'a, T: Incrementable> inc_dec<'a, T> {
    pub fn new() -> inc_dec<'a, T> {
        inc_dec { p: None }
    }

    // [spec:cg3:def:inlines.cg3.inc-dec.inc-fn]
    // [spec:cg3:sem:inlines.cg3.inc-dec.inc-fn]
    pub fn inc(&mut self, pt: &'a mut T) {
        pt.increment();
        self.p = Some(pt);
    }
}

impl<'a, T: Incrementable> Default for inc_dec<'a, T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a, T: Incrementable> Drop for inc_dec<'a, T> {
    // [spec:cg3:def:inlines.cg3.inc-dec.inc-dec-fn]
    // [spec:cg3:sem:inlines.cg3.inc-dec.inc-dec-fn]
    fn drop(&mut self) {
        if let Some(p) = self.p.as_mut() {
            p.decrement();
        }
    }
}

// [spec:cg3:def:inlines.cg3.scope-guard]
// `std::function<void()>` -> `Box<dyn FnMut() + 'a>`. The C++ "empty func +
// good -> throws bad_function_call" case does not arise: a callable is always
// supplied at construction.
pub struct scope_guard<'a> {
    func: Box<dyn FnMut() + 'a>,
    good: bool,
}

impl<'a> scope_guard<'a> {
    pub fn new<F: FnMut() + 'a>(func: F) -> scope_guard<'a> {
        scope_guard {
            func: Box::new(func),
            good: true,
        }
    }

    // [spec:cg3:def:inlines.cg3.scope-guard.set-fn]
    // [spec:cg3:sem:inlines.cg3.scope-guard.set-fn]
    // C++ default `val = true`; caller passes explicitly (no Rust default args).
    pub fn set(&mut self, val: bool) {
        self.good = val;
    }
}

impl<'a> Drop for scope_guard<'a> {
    // [spec:cg3:def:inlines.cg3.scope-guard.scope-guard-fn]
    // [spec:cg3:sem:inlines.cg3.scope-guard.scope-guard-fn]
    fn drop(&mut self) {
        if self.good {
            (self.func)();
        }
    }
}

// ---------------------------------------------------------------------------
// Linked-list reverse, reversed-range iteration, erase, make_array, concat
// ---------------------------------------------------------------------------

// [spec:cg3:def:inlines.cg3.reverse-fn] — see `crate::reading::reverse`:
// wave 4 moved the linked-list reversal onto the arena Reading chain (the only
// linked type in the port), replacing the raw-pointer `Linked` trait + unsafe
// generic with safe id-chain reversal.

// [spec:cg3:def:inlines.cg3.reversed]
pub struct Reversed<'a, T: ?Sized> {
    pub t: &'a T,
}

// [spec:cg3:def:inlines.cg3.reversed-fn]
// [spec:cg3:sem:inlines.cg3.reversed-fn]
pub fn reversed<T: ?Sized>(c: &T) -> Reversed<'_, T> {
    Reversed { t: c }
}

// [spec:cg3:def:inlines.cg3.begin-fn]
// [spec:cg3:sem:inlines.cg3.begin-fn]
// C++ ADL `begin(Reversed<T>)` -> `std::rbegin`. Rust returns a reverse
// iterator over the wrapped container.
pub fn begin<'a, T>(c: Reversed<'a, T>) -> std::iter::Rev<<&'a T as IntoIterator>::IntoIter>
where
    &'a T: IntoIterator,
    <&'a T as IntoIterator>::IntoIter: DoubleEndedIterator,
{
    c.t.into_iter().rev()
}

// [spec:cg3:def:inlines.cg3.end-fn]
// [spec:cg3:sem:inlines.cg3.end-fn]
// C++ ADL `end(Reversed<T>)` -> `std::rend`. Rust has no separate rend type in
// this scheme; an exhausted reverse iterator stands in for the past-the-reverse-
// end position. Prefer the `IntoIterator` impl (below) for real iteration.
pub fn end<'a, T>(c: Reversed<'a, T>) -> std::iter::Rev<<&'a T as IntoIterator>::IntoIter>
where
    &'a T: IntoIterator,
    <&'a T as IntoIterator>::IntoIter: DoubleEndedIterator,
{
    let mut it = c.t.into_iter().rev();
    while it.next().is_some() {}
    it
}

// Makes `for x in reversed(&container)` iterate in reverse (the actual intent of
// the C++ begin/end ADL pair).
impl<'a, T: ?Sized> IntoIterator for Reversed<'a, T>
where
    &'a T: IntoIterator,
    <&'a T as IntoIterator>::IntoIter: DoubleEndedIterator,
{
    type Item = <&'a T as IntoIterator>::Item;
    type IntoIter = std::iter::Rev<<&'a T as IntoIterator>::IntoIter>;
    fn into_iter(self) -> Self::IntoIter {
        self.t.into_iter().rev()
    }
}

// [spec:cg3:def:inlines.cg3.erase-fn]
// [spec:cg3:sem:inlines.cg3.erase-fn]
// Erase-remove idiom over Vec<T>: removes ALL elements equal to `val`.
#[inline]
pub fn erase<T: PartialEq>(cont: &mut Vec<T>, val: &T) {
    cont.retain(|x| x != val);
}

// [spec:cg3:def:inlines.cg3.make-array-helper-fn]
// [spec:cg3:sem:inlines.cg3.make-array-helper-fn]
// The C++ compile-time `std::index_sequence` expansion becomes `array::from_fn`,
// which calls `f(0), f(1), ..., f(N-1)` in order.
#[inline]
pub fn make_array_helper<const N: usize, R, F: Fn(usize) -> R>(f: F) -> [R; N] {
    std::array::from_fn(f)
}

// [spec:cg3:def:inlines.cg3.make-array-fn]
// [spec:cg3:sem:inlines.cg3.make-array-fn]
#[inline]
pub fn make_array<const N: usize, R, F: Fn(usize) -> R>(f: F) -> [R; N] {
    make_array_helper::<N, R, F>(f)
}

pub mod details {
    // [spec:cg3:def:inlines.cg3.details.concat-fn]
    // [spec:cg3:sem:inlines.cg3.details.concat-fn]
    // C++ variadic recursion -> a slice of pieces appended in order.
    pub fn _concat(msg: &mut String, args: &[&str]) {
        for a in args {
            msg.push_str(a);
        }
    }
}

// [spec:cg3:def:inlines.cg3.concat-fn]
// [spec:cg3:sem:inlines.cg3.concat-fn]
// Variadic string builder. Rust has no variadics: the first argument is `value`
// and the remaining pieces are passed as a slice `args`.
pub fn concat(value: &str, args: &[&str]) -> String {
    let mut msg = String::from(value);
    details::_concat(&mut msg, args);
    msg
}
