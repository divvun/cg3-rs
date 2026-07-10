//! Port of `src/AST.hpp` — the parser's Abstract Syntax Tree.
//!
//! Literal, bug-for-bug 1:1 translation (Wave 2). The AST is built while the
//! textual parser walks the grammar source and is later serialized as
//! pseudo-XML (the `--dump-ast` output) by [`print_ast`]; `cg-annotate` also
//! consumes it. The flagged quirks are reproduced rather than fixed.
//!
//! ## Porting-representation decisions (apply throughout this file)
//! * **Source pointers (`ASTNode::b` / `ASTNode::e`).** The C++ `const UChar*`
//!   begin/end pointers delimit the node's span *inside the grammar source
//!   buffer*. They are kept as raw `*const UChar` (`*const char`) so that
//!   [`print_ast`]'s offset arithmetic (`UI32(node.b - b)`) and the
//!   `AST_Grammar` re-basing — where an `#include`d sub-grammar's offsets are
//!   relative to *its own* buffer — translate 1:1 without an extra
//!   buffer/lifetime parameter, exactly as the original. The future textual
//!   parser (which walks `&[char]` with a cursor) obtains these via
//!   `buf.as_ptr().add(pos)`.
//!   - DEVIATION (inherent to the UTF-8 port, see [`crate::types`]): `UChar` is
//!     a `char` (Unicode scalar, 4 bytes), not a UTF-16 code unit, so the `b`/`e`
//!     offsets printed by [`print_ast`] are in **code-point** units, whereas the
//!     C++ prints **UTF-16 code-unit** offsets (its `<!-- b is ... UTF-16 code
//!     unit offset -->` comment). The numbers differ for any text containing
//!     non-BMP / multi-unit characters. Unavoidable given the UTF-8 decision.
//! * **`ASTType_str` name table.** The C++ lazily fills a `thread_local const
//!   char* ASTType_str[]` via the `AST_OPEN` macro (`ASTType_str[AST_##type] =
//!   #type`), leaving never-opened types null. The port replaces it with the
//!   static [`ASTTYPE_STR`] table populated for *every* type. Output is
//!   identical for any node that was opened (which is every printed node), and
//!   it avoids the C++ UB of `%s`-printing a null name for an unopened type.
//! * **`xml_encode` return value.** Returns an owned [`UString`] instead of a
//!   `const UChar*` aliasing a shared `thread_local` scratch buffer — see the
//!   note on [`xml_encode`].
//! * **The `AST_OPEN` / `AST_CLOSE` / `AST_CLOSE_ID` macros + the `cur_ast_help`
//!   self-pointer chain.** See the note on [`ASTHelper`]: the RAII open/close is
//!   modelled by [`ASTHelper::new`] (open), the [`Drop`] impl / [`ASTHelper::destroy`]
//!   (close), and the [`ASTHelper::close`] / [`ASTHelper::close_id`] methods
//!   (the two `AST_CLOSE*` macros). The global `cur_ast_help` pointer is elided.
//! * **Output sink.** `print_ast` writes to a `&mut dyn std::io::Write`
//!   (`u_fprintf(std::ostream&, ...)` → `write!`); like `u_fprintf`, write
//!   errors are ignored.

use std::cell::Cell;
use std::io::Write;
use std::ptr;

use crate::inlines::ui32;
use crate::types::{UChar, UString};

// [spec:cg3:def:ast.ast-type]
/// C++ `enum ASTType`. The concrete C++ name is `ASTType`; kept verbatim (as
/// with `C_OPS` et al.). Default discriminants `0..NUM_ASTTypes` let a node's
/// type index [`ASTTYPE_STR`] via `ty as usize`.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
#[repr(u32)]
pub enum ASTType {
    AST_Unknown,
    AST_AfterSections,
    AST_Anchor,
    AST_AnchorName,
    AST_Barrier,
    AST_BarrierSafe,
    AST_BeforeSections,
    AST_CmdArgs,
    AST_CompositeTag,
    AST_Context,
    AST_ContextMod,
    AST_ContextPos,
    AST_Contexts,
    AST_ContextsTarget,
    AST_Delimiters,
    AST_Grammar,
    AST_Include,
    AST_IncludeFilename,
    AST_List,
    AST_MappingPrefix,
    AST_NullSection,
    AST_Option,
    AST_Options,
    AST_Parentheses,
    AST_PreferredTargets,
    AST_ReopenMappings,
    AST_Rule,
    AST_RuleAddcohortWhere,
    AST_RuleDirection,
    AST_RuleExcept,
    AST_RuleExternalCmd,
    AST_RuleExternalType,
    AST_RuleFlag,
    AST_RuleMaplist,
    AST_RuleMoveType,
    AST_RuleName,
    AST_RuleSublist,
    AST_RuleSubrules,
    AST_RuleTarget,
    AST_RuleType,
    AST_RuleWithChildDepTarget,
    AST_RuleWithChildTarget,
    AST_RuleWordform,
    AST_Section,
    AST_Set,
    AST_SetInline,
    AST_SetName,
    AST_SetOp,
    AST_SoftDelimiters,
    AST_StaticSets,
    AST_UndefSets,
    AST_StrictTags,
    AST_ListTags,
    AST_SubReadings,
    AST_SubReadingsDirection,
    AST_Tag,
    AST_TagList,
    AST_Template,
    AST_TemplateInline,
    AST_TemplateName,
    AST_TemplateRef,
    AST_TemplateShorthand,
    AST_TextDelimiters,
    NUM_ASTTypes,
}

impl Default for ASTType {
    /// Mirrors the C++ default node type (`ASTType type = AST_Unknown` via the
    /// `ASTNode` ctor's default argument).
    fn default() -> Self {
        ASTType::AST_Unknown
    }
}

/// Number of real `ASTType` values (== `NUM_ASTTypes`'s discriminant); the size
/// of [`ASTTYPE_STR`].
pub const NUM_ASTTYPES: usize = ASTType::NUM_ASTTypes as usize;

/// Static analog of the C++ lazily-filled `thread_local ASTType_str[]`. Indexed
/// by `ASTType as usize`; the strings are the `#type` names (i.e. without the
/// `AST_` prefix) that the `AST_OPEN(type)` macro registered. See the module
/// note on the `ASTType_str` deviation.
pub const ASTTYPE_STR: [&str; NUM_ASTTYPES] = [
    "Unknown",
    "AfterSections",
    "Anchor",
    "AnchorName",
    "Barrier",
    "BarrierSafe",
    "BeforeSections",
    "CmdArgs",
    "CompositeTag",
    "Context",
    "ContextMod",
    "ContextPos",
    "Contexts",
    "ContextsTarget",
    "Delimiters",
    "Grammar",
    "Include",
    "IncludeFilename",
    "List",
    "MappingPrefix",
    "NullSection",
    "Option",
    "Options",
    "Parentheses",
    "PreferredTargets",
    "ReopenMappings",
    "Rule",
    "RuleAddcohortWhere",
    "RuleDirection",
    "RuleExcept",
    "RuleExternalCmd",
    "RuleExternalType",
    "RuleFlag",
    "RuleMaplist",
    "RuleMoveType",
    "RuleName",
    "RuleSublist",
    "RuleSubrules",
    "RuleTarget",
    "RuleType",
    "RuleWithChildDepTarget",
    "RuleWithChildTarget",
    "RuleWordform",
    "Section",
    "Set",
    "SetInline",
    "SetName",
    "SetOp",
    "SoftDelimiters",
    "StaticSets",
    "UndefSets",
    "StrictTags",
    "ListTags",
    "SubReadings",
    "SubReadingsDirection",
    "Tag",
    "TagList",
    "Template",
    "TemplateInline",
    "TemplateName",
    "TemplateRef",
    "TemplateShorthand",
    "TextDelimiters",
];

// [spec:cg3:def:ast.ast-node]
/// C++ `struct ASTNode`. A node in the parse tree.
///
/// QUIRK (realloc-invalidation): the children `cs` are stored **by value** in a
/// `Vec<ASTNode>`, exactly as the C++ `std::vector<ASTNode>`. A later push onto
/// an ancestor's `cs` can reallocate that vector and invalidate any raw
/// `*mut ASTNode` (e.g. the thread-local `CUR_AST`) held into it — the same
/// fragility as the original. See [`ASTHelper::new`].
#[derive(Debug)]
pub struct ASTNode {
    /// C++ `ASTType type;` (`type` is a Rust keyword → raw identifier).
    pub r#type: ASTType,
    /// C++ `size_t line = 0;` — 1-based source line number.
    pub line: usize,
    /// C++ `const UChar *b = nullptr;` — begin pointer into the source buffer.
    pub b: *const UChar,
    /// C++ `const UChar *e = nullptr;` — end pointer into the source buffer.
    pub e: *const UChar,
    /// C++ `uint32_t u = 0;` — extra payload (e.g. a dedup id set by `AST_CLOSE_ID`).
    pub u: u32,
    /// C++ `std::vector<ASTNode> cs;` — children stored by value.
    pub cs: Vec<ASTNode>,
}

impl ASTNode {
    // [spec:cg3:def:ast.ast-node.ast-node-fn]
    // [spec:cg3:sem:ast.ast-node.ast-node-fn]
    /// Constructs an `ASTNode`. Member-initializes `type`/`line`/`b`/`e` from
    /// the arguments; `u` defaults to `0` and `cs` to an empty vector. (The C++
    /// ctor supplies defaults `AST_Unknown, 0, nullptr, nullptr`; Rust has no
    /// default arguments, so callers pass them explicitly — `null` for an
    /// as-yet-unset `e`, which `AST_CLOSE` fills in later.)
    pub fn new(r#type: ASTType, line: usize, b: *const UChar, e: *const UChar) -> ASTNode {
        ASTNode {
            r#type,
            line,
            b,
            e,
            u: 0,
            cs: Vec::new(),
        }
    }
}

// --- thread_local parser state (C++ module-level `thread_local` globals) -----
//
// No spec:def id — these are the AST.hpp file-scope thread_locals that back the
// `AST_OPEN`/`AST_CLOSE` machinery. `ast` is kept as a leaked `Box<ASTNode>`
// (thread-lifetime) so that raw pointers into its `cs` tree stay valid for the
// thread, mirroring `thread_local ASTNode ast;` + `cur_ast = &ast`. (Minor: the
// C++ runs the root's destructor at thread exit; the port leaks it instead.)
thread_local! {
    /// C++ `thread_local bool parse_ast = false;` — whether AST building is on.
    static PARSE_AST: Cell<bool> = const { Cell::new(false) };
    /// C++ `thread_local ASTNode ast;` — the tree root (leaked; see above).
    static AST_ROOT: Cell<*mut ASTNode> =
        Cell::new(Box::into_raw(Box::new(ASTNode::new(ASTType::AST_Unknown, 0, ptr::null(), ptr::null()))));
    /// C++ `thread_local ASTNode* cur_ast = &ast;` — the current (innermost) node.
    static CUR_AST: Cell<*mut ASTNode> = Cell::new(AST_ROOT.with(|r| r.get()));
}

/// Sets the thread-local `parse_ast` flag (C++ `parse_ast = _dump_ast;`, done in
/// the `TextualParser` ctor). Enables/disables AST building for the thread.
pub fn set_parse_ast(on: bool) {
    PARSE_AST.with(|p| p.set(on));
}

/// Reads the thread-local `parse_ast` flag.
pub fn parse_ast_enabled() -> bool {
    PARSE_AST.with(|p| p.get())
}

/// Borrows the thread-local AST root (`ast`) — used by the parser's
/// `print_ast` wrapper, which serializes `ast.cs.front()`.
pub fn with_ast_root<R>(f: impl FnOnce(&ASTNode) -> R) -> R {
    AST_ROOT.with(|r| {
        // SAFETY: the root is a live, leaked `Box<ASTNode>` for the thread's lifetime.
        f(unsafe { &*r.get() })
    })
}

/// `node.b - b` in `UChar` (element) units — the pointer subtraction that feeds
/// the C++ `UI32(...)`. Uses a wrapping byte-distance / `size_of` rather than
/// `offset_from` to avoid UB on a null/foreign pointer (which never occurs for
/// an `AST_OPEN`'d node — its `b`/`e` are always set).
fn uchar_offset(p: *const UChar, base: *const UChar) -> usize {
    (p as usize).wrapping_sub(base as usize) / core::mem::size_of::<UChar>()
}

// [spec:cg3:def:ast.xml-encode-fn]
// [spec:cg3:sem:ast.xml-encode-fn]
/// XML-escapes the `[b, e)` `UChar` range and returns it. Escapes exactly five
/// characters — `&`→`&amp;`, `"`→`&quot;`, `'`→`&apos;`, `<`→`&lt;`, `>`→`&gt;`
/// — appending every other code unit verbatim.
///
/// PORT DEVIATION (buffer lifetime): the C++ returns a `const UChar*` aliasing a
/// shared `static thread_local UString buf` that is valid only until the next
/// `xml_encode` call on the thread (callers must consume it — via a single
/// `u_fprintf` — before calling again). That footgun does not translate to safe
/// Rust, so this returns an **owned** [`UString`]; the returned text is
/// identical and callers no longer have the consume-before-reuse constraint.
pub fn xml_encode(b: *const UChar, e: *const UChar) -> UString {
    let mut buf = UString::new();
    // C++ `buf.reserve(e - b)` (element count).
    buf.reserve(uchar_offset(e, b));
    // SAFETY: `[b, e)` is the caller-supplied source span; walked by element,
    // exactly as the C++ `for (; b != e; ++b)`.
    let mut p = b;
    unsafe {
        while p != e {
            match *p {
                '&' => buf.push_str("&amp;"),
                '"' => buf.push_str("&quot;"),
                '\'' => buf.push_str("&apos;"),
                '<' => buf.push_str("&lt;"),
                '>' => buf.push_str("&gt;"),
                c => buf.push(c),
            }
            p = p.add(1);
        }
    }
    buf
}

// [spec:cg3:def:ast.print-ast-fn]
// [spec:cg3:sem:ast.print-ast-fn]
/// Recursively serializes the `node` subtree to `out` as indented pseudo-XML.
/// `b` is the base pointer used to compute character offsets; `n` is the
/// indentation depth (leading spaces). Errors from `out` are ignored, matching
/// `u_fprintf`.
pub fn print_ast(out: &mut dyn Write, b: *const UChar, n: usize, node: &ASTNode) {
    use ASTType::*;

    // C++ `std::string indent(n, ' ');`
    let indent = " ".repeat(n);
    // C++ `ASTType_str[node.type]` (see the ASTTYPE_STR deviation note).
    let name = ASTTYPE_STR[node.r#type as usize];
    // C++ `%s<%s l="%u" b="%u" e="%u"` — offsets in UChar units.
    let _ = write!(
        out,
        "{}<{} l=\"{}\" b=\"{}\" e=\"{}\"",
        indent,
        name,
        ui32(node.line),
        ui32(uchar_offset(node.b, b)),
        ui32(uchar_offset(node.e, b)),
    );
    // Text-bearing node types also emit ` t="<XML-escaped source span>"`.
    if matches!(
        node.r#type,
        AST_AnchorName
            | AST_ContextMod
            | AST_ContextPos
            | AST_IncludeFilename
            | AST_MappingPrefix
            | AST_Option
            | AST_RuleAddcohortWhere
            | AST_RuleDirection
            | AST_RuleExternalCmd
            | AST_RuleExternalType
            | AST_RuleFlag
            | AST_RuleMoveType
            | AST_RuleName
            | AST_RuleType
            | AST_RuleWordform
            | AST_SetName
            | AST_SetOp
            | AST_SubReadingsDirection
            | AST_Tag
            | AST_TemplateName
            | AST_TemplateRef
    ) {
        let _ = write!(out, " t=\"{}\"", xml_encode(node.b, node.e));
    }
    // C++ `if (node.u) { ... " u=\"%u\"" ... }`
    if node.u != 0 {
        let _ = write!(out, " u=\"{}\"", node.u);
    }
    // C++ `if (node.cs.empty()) { "/>\n"; return; }`
    if node.cs.is_empty() {
        let _ = write!(out, "/>\n");
        return;
    }
    let _ = write!(out, ">\n");
    for it in &node.cs {
        if it.r#type == AST_Grammar {
            // Re-base offsets to the `#include`d sub-grammar's own buffer.
            print_ast(out, it.b, n + 1, it);
        } else {
            print_ast(out, b, n + 1, it);
        }
    }
    let _ = write!(out, "{}</{}>\n", indent, name);
}

// [spec:cg3:def:ast.ast-helper]
/// C++ `struct ASTHelper` — the RAII helper (used via the `AST_OPEN` macro)
/// that opens a node as a child of the current one and restores the cursor on
/// close.
///
/// PORT DEVIATION (the `cur_ast_help` self-pointer chain): the C++ ctor does
/// `cur_ast_help = this`, storing the *stack address* of the helper being
/// constructed into a `thread_local ASTHelper* cur_ast_help`, and `AST_CLOSE`
/// closes "the current helper" through that global. Rust move semantics forbid a
/// by-value constructor storing its own not-yet-placed address in a global, so
/// the global `cur_ast_help` chain is **elided**: a close reaches its guard
/// directly through the owning binding ([`ASTHelper::close`] / [`Drop`]) rather
/// than a global pointer. The saved parent node (`c`) drives the restore; `h` is
/// retained only for structural parity with the C++ `{ ASTNode* c; ASTHelper* h; }`
/// and is always null. Under the parser's strictly-nested (LIFO) open/close
/// discipline this is behaviourally identical, including the idempotency that
/// stops a node from being closed twice (`c`/`h` nulled on first close).
pub struct ASTHelper {
    /// C++ `ASTNode* c;` — the saved parent node, restored to `CUR_AST` on close.
    c: *mut ASTNode,
    /// C++ `ASTHelper* h;` — saved `cur_ast_help`; see the deviation note above
    /// (always null in this port).
    h: *mut ASTHelper,
}

impl ASTHelper {
    // [spec:cg3:def:ast.ast-helper.ast-helper-fn]
    // [spec:cg3:sem:ast.ast-helper.ast-helper-fn]
    /// Opens a new AST node as a child of the current node (the `AST_OPEN`
    /// operation). Saves the current cursor, and — if `parse_ast` is enabled —
    /// pushes an `ASTNode(type, line, b, null)` onto the current node's children
    /// and advances `CUR_AST` to it. When AST building is disabled the helper is
    /// **inert** (`c`/`h` null, no node created), and its [`Drop`] / [`destroy`]
    /// skip. `b` marks the node's begin; `e` is filled later by [`close`].
    ///
    /// (The C++ ctor's default argument `const UChar* e = nullptr` is elided —
    /// `AST_OPEN` never passes it; `e` is always null at construction.)
    ///
    /// [`destroy`]: ASTHelper::destroy
    /// [`close`]: ASTHelper::close
    pub fn new(r#type: ASTType, line: usize, b: *const UChar) -> ASTHelper {
        // C++ init list: `c(cur_ast), h(cur_ast_help)`. (`cur_ast_help` is elided;
        // `h` stays null — see the struct deviation note.)
        let c = CUR_AST.with(|p| p.get());
        if !PARSE_AST.with(|p| p.get()) {
            // C++: `if (!parse_ast) { c = nullptr; h = nullptr; return; }`
            return ASTHelper {
                c: ptr::null_mut(),
                h: ptr::null_mut(),
            };
        }
        // C++: `c->cs.push_back(ASTNode(type, line, b, e)); cur_ast = &c->cs.back();`
        //
        // QUIRK (realloc-invalidation): this push — or any later push onto an
        // ancestor's `cs` — can reallocate the `Vec<ASTNode>` and invalidate
        // every raw `*mut ASTNode` held elsewhere (`CUR_AST`, sibling pointers).
        // We grab `&mut cs.last()` immediately after the push, exactly as the C++
        // takes `&c->cs.back()`, but pointers into these vectors are inherently
        // fragile across sibling insertions.
        //
        // SAFETY: `c` is the current node — a live pointer into the thread-local tree.
        unsafe {
            (*c).cs.push(ASTNode::new(r#type, line, b, ptr::null()));
            let child = (*c).cs.last_mut().unwrap() as *mut ASTNode;
            CUR_AST.with(|p| p.set(child));
        }
        ASTHelper {
            c,
            h: ptr::null_mut(),
        }
    }

    // [spec:cg3:def:ast.ast-helper.destroy-fn]
    // [spec:cg3:sem:ast.ast-helper.destroy-fn]
    /// Closes the node this helper opened, restoring the parent context. If
    /// `parse_ast` is disabled this is a no-op. Otherwise restores
    /// `CUR_AST = c` (the parent) and nulls out `c`/`h`, which makes a second
    /// `destroy()`/`Drop` a no-op (the C++ idempotency). (The C++ also restores
    /// `cur_ast_help = h`; that global is elided here — see the struct note.)
    pub fn destroy(&mut self) {
        // C++: `if (!parse_ast) return;`
        if !PARSE_AST.with(|p| p.get()) {
            return;
        }
        // C++: `cur_ast = c; cur_ast_help = h; c = nullptr; h = nullptr;`
        CUR_AST.with(|p| p.set(self.c));
        self.c = ptr::null_mut();
        self.h = ptr::null_mut();
    }

    /// Port of the `AST_CLOSE(p)` macro (a bare macro in AST.hpp — no spec id):
    /// sets the current node's end pointer, then closes it via [`destroy`].
    ///
    /// [`destroy`]: ASTHelper::destroy
    pub fn close(&mut self, e: *const UChar) {
        // C++: `cur_ast->e = (p); cur_ast_help->destroy();`
        // SAFETY: `CUR_AST` is always a live node pointer (root or a tree node).
        CUR_AST.with(|p| unsafe { (*p.get()).e = e });
        self.destroy();
    }

    /// Port of the `AST_CLOSE_ID(p, n)` macro (no spec id): sets the current
    /// node's end pointer and `u` payload, then closes it via [`destroy`].
    ///
    /// [`destroy`]: ASTHelper::destroy
    pub fn close_id(&mut self, e: *const UChar, u: u32) {
        // C++: `cur_ast->e = (p); cur_ast->u = (n); cur_ast_help->destroy();`
        // SAFETY: `CUR_AST` is always a live node pointer.
        CUR_AST.with(|p| unsafe {
            let node = p.get();
            (*node).e = e;
            (*node).u = u;
        });
        self.destroy();
    }
}

impl Drop for ASTHelper {
    /// C++ `~ASTHelper() { if (c || h) destroy(); }` — closes the node on scope
    /// exit unless it was already closed by an explicit `AST_CLOSE`.
    fn drop(&mut self) {
        if !self.c.is_null() || !self.h.is_null() {
            self.destroy();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ASTNode::new member-initializes type/line/b/e, defaulting u=0 and cs empty;
    // xml_encode escapes exactly &,",',<,> over a [b,e) UChar span.
    // [spec:cg3:sem:ast.ast-node.ast-node-fn/test]
    // [spec:cg3:sem:ast.xml-encode-fn/test]
    #[test]
    fn node_ctor_and_xml_encode() {
        // A source buffer to point b/e into (kept alive for the whole test).
        let src: Vec<UChar> = "a&b<c>\"d'e".chars().collect();
        let b = src.as_ptr();
        // SAFETY: within the allocation; e is one-past the last element.
        let e = unsafe { b.add(src.len()) };

        // ASTNode::new: fields set from args; u=0, cs empty.
        let node = ASTNode::new(ASTType::AST_Tag, 7, b, e);
        assert_eq!(node.r#type, ASTType::AST_Tag);
        assert_eq!(node.line, 7);
        assert_eq!(node.u, 0);
        assert!(node.cs.is_empty());
        assert_eq!(node.b, b);
        assert_eq!(node.e, e);

        // xml_encode escapes the five entities and passes everything else through.
        let encoded = xml_encode(b, e);
        assert_eq!(encoded, "a&amp;b&lt;c&gt;&quot;d&apos;e");

        // Empty span (b == e) encodes to empty.
        assert_eq!(xml_encode(b, b), "");
    }

    // print_ast renders a node subtree as indented pseudo-XML, emitting t="..."
    // for text-bearing types and nesting children. Drives ASTNode::new too.
    // [spec:cg3:sem:ast.print-ast-fn/test]
    #[test]
    fn print_ast_renders_tree() {
        // Buffer for offsets and text spans.
        let src: Vec<UChar> = "noun".chars().collect();
        let base = src.as_ptr();
        // SAFETY: within allocation.
        let tag_e = unsafe { base.add(4) };

        // A Tag child (text-bearing -> gets a t="noun" attribute), spanning [0,4).
        let child = ASTNode::new(ASTType::AST_Tag, 2, base, tag_e);
        // A Set parent containing the child; span [0,0) (offsets b=0,e=0).
        let mut parent = ASTNode::new(ASTType::AST_Set, 1, base, base);
        parent.cs.push(child);

        let mut out: Vec<u8> = Vec::new();
        print_ast(&mut out, base, 0, &parent);
        let s = String::from_utf8(out).unwrap();

        // Parent opens with its offsets, then the nested Tag with its text span,
        // then the parent closes.
        assert!(s.contains("<Set l=\"1\" b=\"0\" e=\"0\">"));
        assert!(s.contains(" <Tag l=\"2\" b=\"0\" e=\"4\" t=\"noun\"/>"));
        assert!(s.contains("</Set>"));

        // A childless, non-text node self-closes with "/>".
        let leaf = ASTNode::new(ASTType::AST_Anchor, 3, base, base);
        let mut out2: Vec<u8> = Vec::new();
        print_ast(&mut out2, base, 0, &leaf);
        let s2 = String::from_utf8(out2).unwrap();
        assert_eq!(s2, "<Anchor l=\"3\" b=\"0\" e=\"0\"/>\n");
    }

    // ASTHelper::new opens a node as a child of the current node (when parse_ast is
    // on) and advances the cursor; destroy() closes it, restoring the parent and
    // becoming idempotent. When parse_ast is off the helper is inert.
    // [spec:cg3:sem:ast.ast-helper.ast-helper-fn/test]
    // [spec:cg3:sem:ast.ast-helper.destroy-fn/test]
    #[test]
    fn ast_helper_open_and_close() {
        let src: Vec<UChar> = "x".chars().collect();
        let b = src.as_ptr();

        // Disabled: helper is inert (no node created, c/h null).
        set_parse_ast(false);
        {
            let h = ASTHelper::new(ASTType::AST_Rule, 1, b);
            assert!(h.c.is_null());
        }

        // Enabled: opening pushes a child onto the current (root) node and the
        // cursor advances to it.
        set_parse_ast(true);
        let root_children_before = with_ast_root(|r| r.cs.len());
        {
            let mut h = ASTHelper::new(ASTType::AST_Rule, 5, b);
            assert!(!h.c.is_null()); // saved parent (the root)
            let after = with_ast_root(|r| r.cs.len());
            assert_eq!(after, root_children_before + 1);
            // The newly opened child carries the type/line we passed.
            with_ast_root(|r| {
                let last = r.cs.last().unwrap();
                assert_eq!(last.r#type, ASTType::AST_Rule);
                assert_eq!(last.line, 5);
            });

            // destroy() closes: restores the parent cursor and nulls c/h.
            h.destroy();
            assert!(h.c.is_null());
            // A second destroy() is an idempotent no-op (c already null).
            h.destroy();
            assert!(h.c.is_null());
        }

        // Reset the thread-local flag so other tests aren't affected.
        set_parse_ast(false);
    }
}
