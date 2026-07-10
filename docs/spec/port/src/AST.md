# src/AST.hpp

> [spec:cg3:def:ast.ast-helper]
> struct ASTHelper {
>   ASTNode* c;
>   ASTHelper* h;
> }

> [spec:cg3:def:ast.ast-helper.ast-helper-fn]
> ASTHelper(ASTType type = AST_Unknown, size_t line = 0, const UChar* b = nullptr, const UChar* e = nullptr)

> [spec:cg3:sem:ast.ast-helper.ast-helper-fn]
> RAII helper (used via the `AST_OPEN` macro) that opens a new AST node as
> a child of the current node while the parser is building the tree, and
> records the previous cursor so it can be restored on close. It operates
> on thread_local globals: `parse_ast` (bool, whether AST building is on),
> `cur_ast` (current `ASTNode*`), and `cur_ast_help` (current
> `ASTHelper*`). Steps:
> - Save the current cursor into members: `c = cur_ast`,
>   `h = cur_ast_help`.
> - Set `cur_ast_help = this` (this helper becomes the innermost).
> - If `parse_ast` is false (AST building disabled), null out `c` and `h`
>   and return — the helper becomes inert (its destructor sees `c == h ==
>   nullptr` and skips), and no node is created.
> - Otherwise push a new `ASTNode(type, line, b, e)` onto the current
>   node's children (`c->cs.push_back(...)`), then set
>   `cur_ast = &c->cs.back()` so subsequent opens nest under this new node.
> `b` marks the node's begin pointer into the source; `e` is filled in
> later by `AST_CLOSE`. The paired `destroy()` (called by `AST_CLOSE` or by
> `~ASTHelper`) restores `cur_ast = c` and `cur_ast_help = h`. QUIRK: nodes
> are stored by value in `std::vector<ASTNode> cs`, so a later push_back to
> an ancestor's `cs` can reallocate and invalidate `cur_ast`/`ASTNode*`
> held elsewhere; the code takes `&c->cs.back()` immediately after each
> push, but pointers into these vectors are inherently fragile across
> sibling insertions.

> [spec:cg3:def:ast.ast-helper.destroy-fn]
> void destroy()

> [spec:cg3:sem:ast.ast-helper.destroy-fn]
> Closes the node this helper opened, restoring the parent context. If
> `parse_ast` is false, do nothing and return. Otherwise restore the
> thread_local globals from the saved members: `cur_ast = c` (the parent
> node) and `cur_ast_help = h` (the parent helper), then null out this
> helper's own `c` and `h`. Nulling them makes `destroy()` effectively
> idempotent: `~ASTHelper` only calls `destroy()` when `c || h` is
> non-null, so a node already closed via the `AST_CLOSE` macro is not
> closed again by the destructor. (The `AST_CLOSE`/`AST_CLOSE_ID` macros
> set `cur_ast->e` — and optionally `cur_ast->u` — before invoking
> `destroy()`.) No return value.

> [spec:cg3:def:ast.ast-node]
> struct ASTNode {
>   ASTType type;
>   size_t line = 0;
>   const UChar *b = nullptr, *e = nullptr;
>   uint32_t u = 0;
>   std::vector<ASTNode> cs;
> }

> [spec:cg3:def:ast.ast-node.ast-node-fn]
> ASTNode(ASTType type = AST_Unknown, size_t line = 0, const UChar* b = nullptr, const UChar* e = nullptr)

> [spec:cg3:sem:ast.ast-node.ast-node-fn]
> Constructs an `ASTNode`. Member-initializes `type` (default
> `AST_Unknown`), `line` (source line number, default 0), `b` (begin
> pointer into the source buffer, default null), and `e` (end pointer,
> default null) from the arguments. The remaining members keep their
> in-class defaults: `u = 0` (an extra `uint32_t` payload, e.g. an id set
> later by `AST_CLOSE_ID`) and `cs` an empty `std::vector<ASTNode>` of
> children. `b`/`e` delimit the source-text span this node covers (as raw
> UChar pointers).

> [spec:cg3:def:ast.ast-type]
> enum ASTType {
>   AST_Unknown;
>   AST_AfterSections;
>   AST_Anchor;
>   AST_AnchorName;
>   AST_Barrier;
>   AST_BarrierSafe;
>   AST_BeforeSections;
>   AST_CmdArgs;
>   AST_CompositeTag;
>   AST_Context;
>   AST_ContextMod;
>   AST_ContextPos;
>   AST_Contexts;
>   AST_ContextsTarget;
>   AST_Delimiters;
>   AST_Grammar;
>   AST_Include;
>   AST_IncludeFilename;
>   AST_List;
>   AST_MappingPrefix;
>   AST_NullSection;
>   AST_Option;
>   AST_Options;
>   AST_Parentheses;
>   AST_PreferredTargets;
>   AST_ReopenMappings;
>   AST_Rule;
>   AST_RuleAddcohortWhere;
>   AST_RuleDirection;
>   AST_RuleExcept;
>   AST_RuleExternalCmd;
>   AST_RuleExternalType;
>   AST_RuleFlag;
>   AST_RuleMaplist;
>   AST_RuleMoveType;
>   AST_RuleName;
>   AST_RuleSublist;
>   AST_RuleSubrules;
>   AST_RuleTarget;
>   AST_RuleType;
>   AST_RuleWithChildDepTarget;
>   AST_RuleWithChildTarget;
>   AST_RuleWordform;
>   AST_Section;
>   AST_Set;
>   AST_SetInline;
>   AST_SetName;
>   AST_SetOp;
>   AST_SoftDelimiters;
>   AST_StaticSets;
>   AST_UndefSets;
>   AST_StrictTags;
>   AST_ListTags;
>   AST_SubReadings;
>   AST_SubReadingsDirection;
>   AST_Tag;
>   AST_TagList;
>   AST_Template;
>   AST_TemplateInline;
>   AST_TemplateName;
>   AST_TemplateRef;
>   AST_TemplateShorthand;
>   AST_TextDelimiters;
>   NUM_ASTTypes;
> }

> [spec:cg3:def:ast.print-ast-fn]
> void print_ast(std::ostream& out, const UChar* b, size_t n, const ASTNode& node)

> [spec:cg3:sem:ast.print-ast-fn]
> Recursively serializes the `ASTNode` subtree `node` to `out` as indented
> pseudo-XML. `b` is the base pointer of the source buffer (for computing
> character offsets); `n` is the current indentation depth (number of
> leading spaces). Steps:
> - Build an indent string of `n` spaces.
> - Print the open tag prefix: `{indent}<{name} l="{line}" b="{boff}"
>   e="{eoff}"` where `{name}` is `ASTType_str[node.type]` (the enum name
>   string registered by the `AST_OPEN` macro), `{line}` is `node.line`,
>   `{boff} = UI32(node.b - b)` and `{eoff} = UI32(node.e - b)` (offsets in
>   UChar units from the base pointer). Uses `u_fprintf` (UTF-8 output).
> - If `node.type` is one of a fixed set of text-bearing types
>   (`AST_AnchorName`, `AST_ContextMod`, `AST_ContextPos`,
>   `AST_IncludeFilename`, `AST_MappingPrefix`, `AST_Option`,
>   `AST_RuleAddcohortWhere`, `AST_RuleDirection`, `AST_RuleExternalCmd`,
>   `AST_RuleExternalType`, `AST_RuleFlag`, `AST_RuleMoveType`,
>   `AST_RuleName`, `AST_RuleType`, `AST_RuleWordform`, `AST_SetName`,
>   `AST_SetOp`, `AST_SubReadingsDirection`, `AST_Tag`, `AST_TemplateName`,
>   `AST_TemplateRef`), also print ` t="{text}"` where `{text}` is
>   `xml_encode(node.b, node.e)` — the node's source span, XML-escaped,
>   printed with `%S`.
> - If `node.u != 0`, print ` u="{u}"`.
> - If `node.cs` is empty, print `/>\n` (self-closing) and return.
> - Otherwise print `>\n`, then for each child `it` in `node.cs` recurse
>   with indent `n+1`: if `it.type == AST_Grammar`, pass `it.b` as the new
>   base pointer (so that included sub-grammar's offsets are relative to
>   its own buffer); otherwise pass the same `b`. Finally print the closing
>   `{indent}</{name}>\n`.
> Recursion depth equals tree depth. Offset math assumes `node.b`/`node.e`
> point within the buffer beginning at `b` (except the `AST_Grammar`
> re-basing).

> [spec:cg3:def:ast.xml-encode-fn]
> const UChar* xml_encode(const UChar* b, const UChar* e)

> [spec:cg3:sem:ast.xml-encode-fn]
> XML-escapes the UTF-16 range `[b, e)` and returns a NUL-terminated
> `UChar*` to the result. Uses a `static thread_local UString buf` reused
> across calls: `buf.clear()`, then `buf.reserve(e - b)`. Iterate `b`
> until `b == e`, appending to `buf`: `'&'` -> `&amp;`, `'"'` -> `&quot;`,
> `'\''` -> `&apos;`, `'<'` -> `&lt;`, `'>'` -> `&gt;`; every other code
> unit is appended verbatim. Return `buf.data()`. IMPORTANT LIFETIME: the
> returned pointer aliases the shared thread_local buffer and is valid only
> until the next `xml_encode` call on the same thread, so callers must
> consume it (e.g. via a single `u_fprintf`) before calling `xml_encode`
> again — which `print_ast` does (one call per node). Escapes exactly the
> five listed characters (note it also escapes `'` and `"`, which are only
> strictly required inside attribute values).

