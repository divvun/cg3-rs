# src/cg-annotate.cpp

> [spec:cg3:def:cg-annotate.file-save-fn]
> inline void file_save(fs::path fn, std::string_view data)

> [spec:cg3:sem:cg-annotate.file-save-fn]
> Writes `data` verbatim to the file at path `fn`, truncating/overwriting.
> Opens `std::ofstream(fn.string(), std::ios::binary)`, sets the stream
> exception mask to `badbit | failbit` (so any failure to open or write THROWS
> `std::ios_base::failure` rather than silently failing), then
> `file.write(data.data(), data.size())`. The stream (and file) is closed by
> RAII when the function returns. No return value.

> [spec:cg3:def:cg-annotate.main-fn]
> int main(int argc, char* argv[])

> [spec:cg3:sem:cg-annotate.main-fn]
> Entry point for `cg-annotate`: reads a profiling SQLite database and renders a
> static HTML site that shows each grammar's source with per-rule/per-context
> match/fail coverage highlighting, plus usage-example pages. Positional-only:
> `argv[1]` = profile DB, `argv[2]` = output folder. `argc` is explicitly
> ignored (`(void)argc`).
> Setup: `u_init(&status)` (on non-file-access failure → ICU error +
> `CG3Quit(1)`); `ucnv_setDefaultName("UTF-8")`; `uloc_setDefault("en_US_POSIX",
> &status)`. `Profiler profiler; profiler.read(argv[1])`.
> Output folder: `folder = argv[2]`; if it does not exist and
> `create_directories(folder)` fails, throw `"Output folder did not exist and
> could not be created!"`. `fs::current_path(folder)` (chdir into it).
> `create_directories("rs")` and `create_directories("cs")` (subfolders for
> rule and context example pages).
> Invert strings: build `strings` = `map<size_t id, string_view>` from
> `profiler.strings`. String id 0 is the whole grammar AST XML (all grammars
> concatenated), so `std::string ast{strings[0]}` is that document.
> Split ASTs per grammar: repeatedly `rfind("<Grammar ")` in `ast` (from the
> end): find the matching `"</Grammar>"`, extract the fragment `g` of length
> `(e-sz)+11` (including the closing tag), `erase` it from `ast`; inside `g`
> parse the integer in ` u="..."` as the grammar id and store `asts[id] = g`.
> (So each grammar's own AST fragment is keyed by its grammar id; processed
> last-to-first.)
> Build offset→HTML-tag maps: `gs_tags` = `map<grammar_id, map<byte_offset,
> deque<string>>>`. For each `(gid, ast)` fragment: set `lines_width[gid] =
> floor(log10(number of '\n' in ast)) + 1` (computed but effectively unused —
> line numbers are later printed with a hardcoded `%06zu`). Then scan the
> fragment for every ` l="` occurrence (each element carrying a line attr); for
> each at position `last`:
> - `tag` = the element name = substring from `rfind('<', last)+1` to `last`
>   (e.g. `Rule`, `Context`, `Set`, `Tag`, ...).
> - Parse ` b="..."` → start byte offset `b`; register an opening `<span
>   class="cg-elem cg<tag>">` by `push_back` into `gs_tags[gid][b]`.
> - Parse ` e="..."` → end byte offset `e`; register a closing `</span>` by
>   `push_front` into `gs_tags[gid][e]`.
> - If `tag=="Rule"` or `tag=="Context"`: parse ` u="..."` → id `u`; form
>   `Key{ET_RULE,u}` (or `ET_CONTEXT` for a Context) and look it up in
>   `profiler.entries`. If found, build an `<a href="rs/<id>.html">` (rule) or
>   `cs/<id>.html` (context) anchor. If the entry is a rule, OR no current rule
>   id is set (`rid==0`): append class `entry good` if `num_match!=0` else
>   `entry bad`, plus `<span class="stats">M:<num_match>, F:<num_fail>`.
>   Otherwise (a context nested under a known rule): look up
>   `profiler.rule_contexts[{rid, id}]`; if present and nonzero, class `entry
>   context good` with `M:<count>`, else class `entry context bad` with `M:0`.
>   Close `</span>`, `push_back` the anchor-open into `gs_tags[gid][b]`, and
>   `push_front` `</a>` into `gs_tags[gid][e]`. If the entry is a rule, set
>   `rid = id` (so subsequent contexts attribute to this rule).
> - `++last` and continue the scan.
> `write_grammar(g, UnicodeString& grammar)` lambda renders one grammar page:
> builds an HTML head via `sprintf` into a resized `html` buffer with title
> `<xml_encode(filename(strings[g]))> annotated` and a `<div id="grammar"
> class="p-2 pre-wrap">`; takes `gz = profiler.grammars[g]` and `tags =
> gs_tags[gz]`; emits a leading `<span class="ln">%06zu</span>` line-number span
> for line 1, then walks the ordered `(offset, taglist)` entries: for each, take
> the UTF-16 substring of `grammar` between the previous offset and this one
> (`tempSubStringBetween`), convert to UTF-8, `xml_encode`, and append char by
> char (emitting a new `%06zu` line-number span on every `'\n'`), then append
> each registered tag string at that offset. Closes the div/body/html and
> `file_save`s to `g<profiler.grammars[g]>.html`.
> Build UTF-16 grammar copies: for each `(fid, gid)` in `profiler.grammars`,
> `grammars[gid] = UnicodeString::fromUTF8(strings[gid])` and call
> `write_grammar(fid, grammars[gid])`.
> `write_entry(id, Entry& e)` lambda renders a usage-example page: from
> `grammars[e.grammar]` take the UTF-16 substring `[e.b, e.e)` (the rule/context
> source), UTF-8 + `xml_encode` it into a `<div id="what">`, then
> `xml_encode(strings[e.example_window])` into a `<div id="context">`; `file_save`
> to `rs/<id>.html` (rule) or `cs/<id>.html` (if `e.type==ET_CONTEXT`).
> For each `(key, entry)` in `profiler.entries`: if `entry.example_window` is
> nonzero, call `write_entry(key.id, entry)`.
> Build `index.html`: for each `(fid, gid)` in `profiler.grammars` append `<li><a
> href="g<gid>.html"><xml_encode(filename(strings[fid]))></a></li>`; wrap in a
> full HTML page whose title uses the FIRST grammar's filename
> (`strings[profiler.grammars.begin()->first]`); `file_save("index.html", ...)`.
> Finally `file_save("style.css", <embedded CSS>)` with the fixed stylesheet.
> No explicit return (0).
> QUIRK (faithfulness): pages are built by `sprintf` into a buffer pre-sized
> with a fixed slack (e.g. `256 + name.size()`); a sufficiently long
> xml-encoded filename could overflow it. All parsing of the AST is done with
> raw `find`/`std::stoul`/`std::stoi` on the XML text (no real XML parser), so
> it depends on the exact attribute formatting emitted by `TextualParser::
> print_ast`.

> [spec:cg3:def:cg-annotate.xml-encode-fn]
> inline auto xml_encode(std::string_view in)

> [spec:cg3:sem:cg-annotate.xml-encode-fn]
> Returns a copy of `in` with the five XML metacharacters escaped as entities.
> Allocates a `std::string buf` reserved to `in.size()`, then iterates `in`
> byte by byte: `'&'`→`"&amp;"`, `'"'`→`"&quot;"`, `'\''`→`"&apos;"`,
> `'<'`→`"&lt;"`, `'>'`→`"&gt;"`; every other byte is appended unchanged.
> Returns `buf`. Operates on raw bytes, so UTF-8 multibyte sequences pass
> through untouched (none of their bytes equal these ASCII characters). Two
> convenience overloads (not this def) forward to it: one taking `std::string`,
> one taking `fs::path` (via `p.string()`).

