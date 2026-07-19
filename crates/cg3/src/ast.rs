//! Port of `src/AST.hpp` — the parser's Abstract Syntax Tree.
//!
//! Literal, bug-for-bug 1:1 translation (Wave 2). The AST is built while the
//! textual parser walks the grammar source and is later serialized as
//! pseudo-XML (the `--dump-ast` output) by [`print_ast`]; `cg-annotate` also
//! consumes it. The flagged quirks are reproduced rather than fixed.
//!
//! ## Porting-representation decisions (apply throughout this file)
//! * **Source spans (`ASTNode::b` / `ASTNode::e` + `ASTNode::buf`).** The C++
//!   `const UChar*` begin/end pointers delimit the node's span *inside the
//!   grammar source buffer*. The port stores them as `usize` **offsets** (`b`,
//!   `e`) into the node's owning buffer plus a shared handle to that buffer
//!   (`buf: Rc<[char]>`). [`print_ast`]'s offset arithmetic (`UI32(node.b - b)`)
//!   becomes the identical subtraction on offsets, and the `AST_Grammar`
//!   re-basing — where an `#include`d sub-grammar's offsets are relative to
//!   *its own* buffer — is preserved by carrying each node's own `buf`. The
//!   textual parser (which walks `&[char]` with a cursor) supplies the offset
//!   directly (`pos`) and clones the shared buffer handle; this replaces the
//!   original's raw `buf.as_ptr().add(pos)` with no observable change.
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

use std::io::Write;
use std::rc::Rc;

use crate::inlines::ui32;
use crate::types::{UChar, UString};

/// Shared, immutable handle to a grammar source buffer. The parser's
/// `grammarbufs` entries and every [`ASTNode`] span reference one of these; a
/// clone is a refcount bump, so associating a span with its buffer is cheap.
pub type SrcBuf = Rc<[char]>;

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
/// The children `cs` are stored **by value** in a `Vec<ASTNode>`, exactly as
/// the C++ `std::vector<ASTNode>`. (The C++ realloc-invalidation fragility —
/// a push invalidating the raw `cur_ast` pointer — does not exist here: the
/// [`Ast`] cursor is an index path, not a pointer.)
#[derive(Debug)]
pub struct ASTNode {
    /// C++ `ASTType type;` (`type` is a Rust keyword → raw identifier).
    pub r#type: ASTType,
    /// C++ `size_t line = 0;` — 1-based source line number.
    pub line: usize,
    /// C++ `const UChar *b = nullptr;` — begin **offset** into [`buf`](Self::buf).
    pub b: usize,
    /// C++ `const UChar *e = nullptr;` — end **offset** into [`buf`](Self::buf)
    /// (`usize::MAX` marks the not-yet-set state the C++ null `e` had before
    /// `AST_CLOSE` fills it in).
    pub e: usize,
    /// The grammar source buffer this node's `[b, e)` span points into. Replaces
    /// the C++ pointers' implicit buffer identity; carried per-node so the
    /// `AST_Grammar` re-basing (an `#include`d sub-grammar in its own buffer)
    /// keeps working. A cheap-to-clone [`Rc`] handle.
    pub buf: SrcBuf,
    /// C++ `uint32_t u = 0;` — extra payload (e.g. a dedup id set by `AST_CLOSE_ID`).
    pub u: u32,
    /// C++ `std::vector<ASTNode> cs;` — children stored by value.
    pub cs: Vec<ASTNode>,
}

/// Sentinel for an as-yet-unset end offset (the C++ null `e` before `AST_CLOSE`).
pub const AST_E_UNSET: usize = usize::MAX;

impl ASTNode {
    // [spec:cg3:def:ast.ast-node.ast-node-fn]
    // [spec:cg3:sem:ast.ast-node.ast-node-fn]
    /// Constructs an `ASTNode`. Member-initializes `type`/`line`/`b`/`e` from
    /// the arguments; `u` defaults to `0` and `cs` to an empty vector. (The C++
    /// ctor supplies defaults `AST_Unknown, 0, nullptr, nullptr`; Rust has no
    /// default arguments, so callers pass them explicitly — [`AST_E_UNSET`] for
    /// an as-yet-unset `e`, which `AST_CLOSE` fills in later.)
    pub fn new(r#type: ASTType, line: usize, b: usize, e: usize, buf: SrcBuf) -> ASTNode {
        ASTNode {
            r#type,
            line,
            b,
            e,
            buf,
            u: 0,
            cs: Vec::new(),
        }
    }
}

// --- parser-owned AST builder (C++ module-level `thread_local` globals) ------
//
// No spec:def id — this replaces the AST.hpp file-scope thread_locals
// (`thread_local bool parse_ast`, `thread_local ASTNode ast`, `thread_local
// ASTNode* cur_ast`) that backed the `AST_OPEN`/`AST_CLOSE` machinery. Wave 4:
// the builder is a value OWNED by [`crate::textual_parser::TextualParser`], the
// root is a plain owned field (no leaked `Box`), and the raw-pointer cursor is
// an index PATH into the tree (`Vec<usize>` of child indices), which stays
// valid across `Vec<ASTNode>` reallocations — eliminating the C++
// realloc-invalidation fragility along with the globals.
pub struct Ast {
    /// C++ `thread_local bool parse_ast;` — whether AST building is on.
    enabled: bool,
    /// C++ `thread_local ASTNode ast;` — the tree root, owned.
    root: ASTNode,
    /// C++ `thread_local ASTNode* cur_ast;` — as an index path from the root:
    /// empty ⇒ the root itself; otherwise each element selects a child in the
    /// previous node's `cs`.
    cursor: Vec<usize>,
}

impl Ast {
    /// C++ `parse_ast = _dump_ast;` (done in the `TextualParser` ctor) fused
    /// with the implicit `thread_local` root/cursor initialization.
    pub fn new(enabled: bool) -> Ast {
        Ast {
            enabled,
            root: ASTNode::new(
                ASTType::AST_Unknown,
                0,
                0,
                AST_E_UNSET,
                Rc::from([] as [char; 0]),
            ),
            cursor: Vec::new(),
        }
    }

    /// Whether AST building is on (C++ reads of `parse_ast`).
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// C++ `parse_ast = true;` — profiling enables AST building after
    /// construction (see `TextualParser::parse_from_u_char`).
    pub fn set_enabled(&mut self, on: bool) {
        self.enabled = on;
    }

    /// Current cursor depth (0 = the root). Used with [`truncate_cursor`] to
    /// mimic the C++ `ASTHelper` RAII unwinding on a parse error.
    ///
    /// [`truncate_cursor`]: Ast::truncate_cursor
    pub fn cursor_depth(&self) -> usize {
        self.cursor.len()
    }

    /// Pop the cursor back to `depth` — the analogue of C++ stack unwinding
    /// running every in-scope `~ASTHelper()` when a parse error is thrown.
    pub fn truncate_cursor(&mut self, depth: usize) {
        self.cursor.truncate(depth);
    }

    /// The tree root (`ast`) — the parser's `print_ast` wrapper serializes
    /// `root().cs.front()`.
    pub fn root(&self) -> &ASTNode {
        &self.root
    }

    /// Resolve the cursor path to the current (innermost) node.
    fn current_mut(&mut self) -> &mut ASTNode {
        let mut node = &mut self.root;
        for &i in &self.cursor {
            node = &mut node.cs[i];
        }
        node
    }
}

// [spec:cg3:def:ast.xml-encode-fn]
// [spec:cg3:sem:ast.xml-encode-fn]
/// XML-escapes the `[b, e)` `UChar` range of `src` and returns it. Escapes
/// exactly five characters — `&`→`&amp;`, `"`→`&quot;`, `'`→`&apos;`, `<`→`&lt;`,
/// `>`→`&gt;` — appending every other code unit verbatim.
///
/// PORT DEVIATION (span form): the C++ takes two `const UChar*` (`b`, `e`) into a
/// live buffer; the port passes the already-sliced `[b, e)` span (`src`) —
/// identical elements, walked one-by-one exactly as the C++ `for (; b != e; ++b)`.
///
/// PORT DEVIATION (buffer lifetime): the C++ returns a `const UChar*` aliasing a
/// shared `static thread_local UString buf` that is valid only until the next
/// `xml_encode` call on the thread (callers must consume it — via a single
/// `u_fprintf` — before calling again). That footgun does not translate to safe
/// Rust, so this returns an **owned** [`UString`]; the returned text is
/// identical and callers no longer have the consume-before-reuse constraint.
pub fn xml_encode(src: &[UChar]) -> UString {
    let mut buf = UString::new();
    // C++ `buf.reserve(e - b)` (element count).
    buf.reserve(src.len());
    for &c in src {
        match c {
            '&' => buf.push_str("&amp;"),
            '"' => buf.push_str("&quot;"),
            '\'' => buf.push_str("&apos;"),
            '<' => buf.push_str("&lt;"),
            '>' => buf.push_str("&gt;"),
            c => buf.push(c),
        }
    }
    buf
}

// [spec:cg3:def:ast.print-ast-fn]
// [spec:cg3:sem:ast.print-ast-fn]
/// Recursively serializes the `node` subtree to `out` as indented pseudo-XML.
/// `base` is the base **offset** subtracted to yield each node's printed
/// character offset (C++'s base pointer `b`); `n` is the indentation depth
/// (leading spaces). Errors from `out` are ignored, matching `u_fprintf`.
pub fn print_ast(out: &mut dyn Write, base: usize, n: usize, node: &ASTNode) {
    use ASTType::*;

    // C++ `std::string indent(n, ' ');`
    let indent = " ".repeat(n);
    // C++ `ASTType_str[node.type]` (see the ASTTYPE_STR deviation note).
    let name = ASTTYPE_STR[node.r#type as usize];
    // C++ `%s<%s l="%u" b="%u" e="%u"` — offsets in UChar units (`node.b - base`).
    let _ = write!(
        out,
        "{}<{} l=\"{}\" b=\"{}\" e=\"{}\"",
        indent,
        name,
        ui32(node.line),
        ui32(node.b.wrapping_sub(base)),
        ui32(node.e.wrapping_sub(base)),
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
        // C++ `xml_encode(node.b, node.e)` — the `[b, e)` span of the node's own
        // buffer. Text-bearing printed nodes are always closed, so `e` is set;
        // the `AST_E_UNSET` guard makes an unclosed node's span empty (the C++
        // would have read from a null `e`, which never occurs here).
        let end = if node.e == AST_E_UNSET {
            node.b
        } else {
            node.e
        };
        let _ = write!(out, " t=\"{}\"", xml_encode(&node.buf[node.b..end]));
    }
    // C++ `if (node.u) { ... " u=\"%u\"" ... }`
    if node.u != 0 {
        let _ = write!(out, " u=\"{}\"", node.u);
    }
    // C++ `if (node.cs.empty()) { "/>\n"; return; }`
    if node.cs.is_empty() {
        let _ = writeln!(out, "/>");
        return;
    }
    let _ = writeln!(out, ">");
    for it in &node.cs {
        if it.r#type == AST_Grammar {
            // Re-base offsets to the `#include`d sub-grammar's own buffer.
            print_ast(out, it.b, n + 1, it);
        } else {
            print_ast(out, base, n + 1, it);
        }
    }
    let _ = writeln!(out, "{}</{}>", indent, name);
}

// [spec:cg3:def:ast.ast-helper]
/// C++ `struct ASTHelper` — the helper (used via the `AST_OPEN` macro) that
/// opens a node as a child of the current one and restores the cursor on close.
///
/// PORT SHAPE (wave 4): the C++ helper captured two `thread_local` globals
/// (`cur_ast` and the `cur_ast_help` self-pointer chain). Both globals are
/// gone: the helper now operates on the caller's [`Ast`] builder, passed
/// explicitly to [`new`](ASTHelper::new)/[`destroy`](ASTHelper::destroy)/
/// [`close`](ASTHelper::close)/[`close_id`](ASTHelper::close_id), and holds
/// only its open/closed state. Under the parser's strictly-nested (LIFO)
/// open/close discipline this is behaviourally identical to the C++, including
/// the idempotency that stops a node from being closed twice (the C++ nulled
/// `c`/`h` on first close; here `open` flips false). The C++ destructor's
/// close-on-scope-exit fallback is unreachable in the port (the single open
/// site always closes explicitly) and is not reproduced.
pub struct ASTHelper {
    /// Whether this helper currently has an open node (C++ `c != nullptr`);
    /// false when inert (`parse_ast` off) or already closed.
    open: bool,
}

impl ASTHelper {
    // [spec:cg3:def:ast.ast-helper.ast-helper-fn]
    // [spec:cg3:sem:ast.ast-helper.ast-helper-fn]
    /// Opens a new AST node as a child of the current node (the `AST_OPEN`
    /// operation). If AST building is enabled, pushes an
    /// `ASTNode(type, line, b, unset)` onto the current node's children and
    /// advances the cursor to it. When AST building is disabled the helper is
    /// **inert** (no node created) and [`destroy`] skips. `b` is the begin
    /// **offset** into `buf` (the node's owning grammar buffer); `e` is filled
    /// later by [`close`].
    ///
    /// (The C++ ctor's default argument `const UChar* e = nullptr` is elided —
    /// `AST_OPEN` never passes it; `e` is always unset at construction.)
    ///
    /// [`destroy`]: ASTHelper::destroy
    /// [`close`]: ASTHelper::close
    pub fn new(ast: &mut Ast, r#type: ASTType, line: usize, b: usize, buf: SrcBuf) -> ASTHelper {
        // C++: `if (!parse_ast) { c = nullptr; h = nullptr; return; }`
        if !ast.enabled {
            return ASTHelper { open: false };
        }
        // C++: `c->cs.push_back(ASTNode(type, line, b, e)); cur_ast = &c->cs.back();`
        // The index-path cursor stays valid across `cs` reallocations (the C++
        // raw-pointer version could dangle — see the module note).
        let cur = ast.current_mut();
        cur.cs.push(ASTNode::new(r#type, line, b, AST_E_UNSET, buf));
        let idx = cur.cs.len() - 1;
        ast.cursor.push(idx);
        ASTHelper { open: true }
    }

    // [spec:cg3:def:ast.ast-helper.destroy-fn]
    // [spec:cg3:sem:ast.ast-helper.destroy-fn]
    /// Closes the node this helper opened, restoring the parent context. If
    /// `parse_ast` is disabled this is a no-op. Otherwise pops the cursor back
    /// to the parent and marks the helper closed, which makes a second
    /// `destroy()` a no-op (the C++ idempotency).
    pub fn destroy(&mut self, ast: &mut Ast) {
        // C++: `if (!parse_ast) return;` — and the closed-idempotency guard.
        if !ast.enabled || !self.open {
            return;
        }
        // C++: `cur_ast = c; cur_ast_help = h; c = nullptr; h = nullptr;`
        ast.cursor.pop();
        self.open = false;
    }

    /// Port of the `AST_CLOSE(p)` macro (a bare macro in AST.hpp — no spec id):
    /// sets the current node's end **offset**, then closes it via [`destroy`].
    ///
    /// [`destroy`]: ASTHelper::destroy
    pub fn close(&mut self, ast: &mut Ast, e: usize) {
        // C++: `cur_ast->e = (p); cur_ast_help->destroy();`
        if ast.enabled {
            ast.current_mut().e = e;
        }
        self.destroy(ast);
    }

    /// Port of the `AST_CLOSE_ID(p, n)` macro (no spec id): sets the current
    /// node's end **offset** and `u` payload, then closes it via [`destroy`].
    ///
    /// [`destroy`]: ASTHelper::destroy
    pub fn close_id(&mut self, ast: &mut Ast, e: usize, u: u32) {
        // C++: `cur_ast->e = (p); cur_ast->u = (n); cur_ast_help->destroy();`
        if ast.enabled {
            let node = ast.current_mut();
            node.e = e;
            node.u = u;
        }
        self.destroy(ast);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ASTNode::new member-initializes type/line/b/e/buf, defaulting u=0 and cs
    // empty; xml_encode escapes exactly &,",',<,> over a [b,e) UChar span.
    // [spec:cg3:sem:ast.ast-node.ast-node-fn/test]
    // [spec:cg3:sem:ast.xml-encode-fn/test]
    #[test]
    fn node_ctor_and_xml_encode() {
        // A source buffer the b/e offsets index into.
        let src: SrcBuf = Rc::from("a&b<c>\"d'e".chars().collect::<Vec<UChar>>().as_slice());
        let (b, e) = (0usize, src.len());

        // ASTNode::new: fields set from args; u=0, cs empty.
        let node = ASTNode::new(ASTType::AST_Tag, 7, b, e, src.clone());
        assert_eq!(node.r#type, ASTType::AST_Tag);
        assert_eq!(node.line, 7);
        assert_eq!(node.u, 0);
        assert!(node.cs.is_empty());
        assert_eq!(node.b, b);
        assert_eq!(node.e, e);

        // xml_encode escapes the five entities and passes everything else through.
        let encoded = xml_encode(&src[b..e]);
        assert_eq!(encoded, "a&amp;b&lt;c&gt;&quot;d&apos;e");

        // Empty span (b == e) encodes to empty.
        assert_eq!(xml_encode(&src[b..b]), "");
    }

    // print_ast renders a node subtree as indented pseudo-XML, emitting t="..."
    // for text-bearing types and nesting children. Drives ASTNode::new too.
    // [spec:cg3:sem:ast.print-ast-fn/test]
    #[test]
    fn print_ast_renders_tree() {
        // Buffer for offsets and text spans.
        let src: SrcBuf = Rc::from("noun".chars().collect::<Vec<UChar>>().as_slice());

        // A Tag child (text-bearing -> gets a t="noun" attribute), spanning [0,4).
        let child = ASTNode::new(ASTType::AST_Tag, 2, 0, 4, src.clone());
        // A Set parent containing the child; span [0,0) (offsets b=0,e=0).
        let mut parent = ASTNode::new(ASTType::AST_Set, 1, 0, 0, src.clone());
        parent.cs.push(child);

        let mut out: Vec<u8> = Vec::new();
        print_ast(&mut out, 0, 0, &parent);
        let s = String::from_utf8(out).unwrap();

        // Parent opens with its offsets, then the nested Tag with its text span,
        // then the parent closes.
        assert!(s.contains("<Set l=\"1\" b=\"0\" e=\"0\">"));
        assert!(s.contains(" <Tag l=\"2\" b=\"0\" e=\"4\" t=\"noun\"/>"));
        assert!(s.contains("</Set>"));

        // A childless, non-text node self-closes with "/>".
        let leaf = ASTNode::new(ASTType::AST_Anchor, 3, 0, 0, src.clone());
        let mut out2: Vec<u8> = Vec::new();
        print_ast(&mut out2, 0, 0, &leaf);
        let s2 = String::from_utf8(out2).unwrap();
        assert_eq!(s2, "<Anchor l=\"3\" b=\"0\" e=\"0\"/>\n");
    }

    // ASTHelper::new opens a node as a child of the Ast builder's current node
    // (when AST building is on) and advances the cursor; destroy() closes it,
    // restoring the parent and becoming idempotent. When building is off the
    // helper is inert. (Wave 4: the builder is caller-owned — no thread_locals.)
    // [spec:cg3:sem:ast.ast-helper.ast-helper-fn/test]
    // [spec:cg3:sem:ast.ast-helper.destroy-fn/test]
    #[test]
    fn ast_helper_open_and_close() {
        let src: SrcBuf = Rc::from("x".chars().collect::<Vec<UChar>>().as_slice());

        // Disabled: helper is inert (no node created).
        let mut ast_off = Ast::new(false);
        {
            let h = ASTHelper::new(&mut ast_off, ASTType::AST_Rule, 1, 0, src.clone());
            assert!(!h.open);
            assert!(ast_off.root().cs.is_empty());
        }

        // Enabled: opening pushes a child onto the current (root) node and the
        // cursor advances to it.
        let mut ast = Ast::new(true);
        {
            let mut h = ASTHelper::new(&mut ast, ASTType::AST_Rule, 5, 0, src.clone());
            assert!(h.open);
            assert_eq!(ast.root().cs.len(), 1);
            // The newly opened child carries the type/line we passed.
            let last = ast.root().cs.last().unwrap();
            assert_eq!(last.r#type, ASTType::AST_Rule);
            assert_eq!(last.line, 5);
            // A nested open lands under the first node (cursor advanced).
            let mut h2 = ASTHelper::new(&mut ast, ASTType::AST_Tag, 6, 0, src.clone());
            assert_eq!(ast.root().cs[0].cs.len(), 1);
            // close(e) fills the current node's end offset and pops back.
            h2.close(&mut ast, 1);
            assert_ne!(ast.root().cs[0].cs[0].e, AST_E_UNSET);

            // destroy() closes: restores the parent cursor and goes inert.
            h.destroy(&mut ast);
            assert!(!h.open);
            // A second destroy() is an idempotent no-op.
            h.destroy(&mut ast);
            assert!(!h.open);
        }

        // close_id sets both the end offset and the u payload.
        let mut ast2 = Ast::new(true);
        let mut g = ASTHelper::new(&mut ast2, ASTType::AST_Grammar, 1, 0, src.clone());
        g.close_id(&mut ast2, 1, 42);
        assert_eq!(ast2.root().cs[0].u, 42);
    }
}
