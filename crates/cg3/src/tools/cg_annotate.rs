//! Port of `src/cg-annotate.cpp` — generate an HTML grammar-annotation report
//! from a profiler database.
//!
//! `argv`: `[prog, profile_db, out_folder]`. Reads the profiling DB, then emits
//! (into `out_folder`) one `g<N>.html` per grammar with per-rule/per-context
//! coverage highlighting, `rs/<id>.html` / `cs/<id>.html` usage-example pages,
//! an `index.html`, and a `style.css`. LIVE flow (profiler read + filesystem +
//! string templating).
//!
//! ## UTF-16 offset parity (NOTE)
//! The profiler records rule/context source spans (`b`/`e`) and the grammar-AST
//! `b`/`e` attributes as ABSOLUTE UTF-16 code-unit offsets (see the AST printer).
//! The C++ therefore keeps a `UnicodeString` (UTF-16) copy of each grammar and
//! slices it by those offsets. This port reproduces that by holding a
//! `Vec<u16>` (UTF-16) copy of each grammar string and slicing it by the same
//! offsets, decoding each slice back to UTF-8 for output — byte-for-byte
//! equivalent to the C++ `tempSubString*` extraction for the offsets these
//! attributes carry.

use std::collections::BTreeMap;
use std::collections::VecDeque;

use crate::profiler::{Profiler, ET_CONTEXT, ET_RULE};

// [spec:cg3:def:cg-annotate.xml-encode-fn]
// [spec:cg3:sem:cg-annotate.xml-encode-fn]
/// C++ `inline auto xml_encode(std::string_view in)`. Escapes the five XML
/// metacharacters; everything else is copied verbatim (byte-wise).
fn xml_encode(in_: &str) -> String {
    let mut buf = String::with_capacity(in_.len());
    for c in in_.bytes() {
        match c {
            b'&' => buf.push_str("&amp;"),
            b'"' => buf.push_str("&quot;"),
            b'\'' => buf.push_str("&apos;"),
            b'<' => buf.push_str("&lt;"),
            b'>' => buf.push_str("&gt;"),
            _ => buf.push(c as char),
        }
    }
    buf
}

// [spec:cg3:def:cg-annotate.file-save-fn]
// [spec:cg3:sem:cg-annotate.file-save-fn]
/// C++ `inline void file_save(fs::path fn, std::string_view data)`. Writes `data`
/// to `fn` (binary), throwing on badbit/failbit. The port surfaces I/O errors via
/// an `expect` (the faithful analogue of the C++ ostream-exception throw).
fn file_save(path: &std::path::Path, data: &str) {
    std::fs::write(path, data.as_bytes())
        .unwrap_or_else(|e| panic!("cg-annotate: failed to write {}: {}", path.display(), e));
}

/// `fs::path(p).filename()` — the trailing component, as a string.
fn path_filename(p: &str) -> String {
    std::path::Path::new(p)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// UTF-16 copy of a grammar string, for offset-addressed substring extraction.
fn to_utf16(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

/// Decode a UTF-16 slice `[b, e)` back to UTF-8 (C++ `tempSubString`/`Between` +
/// `toUTF8String`). Offsets are clamped to the slice bounds.
fn utf16_slice(u: &[u16], b: usize, e: usize) -> String {
    let b = b.min(u.len());
    let e = e.min(u.len()).max(b);
    String::from_utf16_lossy(&u[b..e])
}

// [spec:cg3:def:cg-annotate.main-fn]
// [spec:cg3:sem:cg-annotate.main-fn]
/// C++ `int main(int argc, char* argv[])`.
pub fn main_annotate(args: &[String]) -> i32 {
    // ICU init / codepage / locale dropped (UTF-8 port).

    // Profiler profiler; profiler.read(argv[1]);
    let mut profiler = Profiler::default();
    let _ = profiler.read(&args[1]);

    // fs::path folder(argv[2]); create + chdir.
    let folder = std::path::PathBuf::from(&args[2]);
    if !folder.exists() && std::fs::create_dir_all(&folder).is_err() {
        panic!("Output folder did not exist and could not be created!");
    }
    std::env::set_current_dir(&folder)
        .unwrap_or_else(|e| panic!("cg-annotate: cannot chdir to output folder: {}", e));

    // fs::create_directories("rs"); fs::create_directories("cs");
    let _ = std::fs::create_dir_all("rs");
    let _ = std::fs::create_dir_all("cs");

    // Re-map strings to enable lookup by ID: std::map<size_t, string_view>.
    let strings: BTreeMap<usize, String> =
        profiler.strings.iter().map(|(k, &v)| (v, k.clone())).collect();
    let sget = |id: usize| -> String { strings.get(&id).cloned().unwrap_or_default() };

    // Store per-grammar ASTs separately.
    // std::string ast{strings[0]}; while ((sz = ast.rfind("<Grammar ")) != npos) {...}
    let mut asts: BTreeMap<usize, String> = BTreeMap::new();
    let mut ast = sget(0);
    while let Some(sz) = ast.rfind("<Grammar ") {
        // auto e = ast.find("</Grammar>", sz);
        let e = ast[sz..].find("</Grammar>").map(|p| p + sz).unwrap();
        // auto g = ast.substr(sz, (e - sz) + 11);
        let g = ast[sz..(e + 11).min(ast.len())].to_string();
        // ast.erase(sz, (e - sz) + 11);
        let erase_end = (e + 11).min(ast.len());
        ast.replace_range(sz..erase_end, "");
        // sz = g.find(" u=\"") + 4; e = stoi(g.substr(sz, g.find("\"", sz) - sz));
        let usz = g.find(" u=\"").unwrap() + 4;
        let uend = g[usz..].find('"').unwrap() + usz;
        let gid: usize = g[usz..uend].parse().unwrap();
        asts.insert(gid, g);
    }

    // Extract each rule and context's start offset, per grammar.
    // std::map<size_t, std::map<size_t, std::deque<std::string>>> gs_tags;
    let mut gs_tags: BTreeMap<usize, BTreeMap<usize, VecDeque<String>>> = BTreeMap::new();
    let mut lines_width: BTreeMap<usize, usize> = BTreeMap::new();

    for (&git, ast) in &asts {
        // lines_width[it.first] = log10(count('\n')) + 1;
        let nl = ast.matches('\n').count();
        lines_width.insert(git, ((nl as f64).log10() + 1.0) as usize);

        let mut last: usize = 0;
        let mut rid: u32 = 0;

        // while ((last = ast.find(" l=\"", last)) != npos)
        while let Some(found) = ast[last..].find(" l=\"") {
            last += found;

            // tagoff = ast.rfind("<", last) + 1; tag = view(&ast[tagoff], last - tagoff);
            let tagoff = ast[..last].rfind('<').unwrap() + 1;
            let tag = ast[tagoff..last].to_string();

            // b = stoul(after " b=\"")
            let bpos = ast[last..].find(" b=\"").unwrap() + last + 4;
            let bend = ast[bpos..].find('"').unwrap() + bpos;
            let b: usize = ast[bpos..bend].parse().unwrap();
            let mut html = String::new();
            html.push_str("<span class=\"cg-elem cg");
            html.push_str(&tag);
            html.push_str("\">");
            gs_tags.entry(git).or_default().entry(b).or_default().push_back(html.clone());

            // e = stoul(after " e=\"")
            let epos = ast[last..].find(" e=\"").unwrap() + last + 4;
            let eend = ast[epos..].find('"').unwrap() + epos;
            let e: usize = ast[epos..eend].parse().unwrap();
            html.clear();
            html.push_str("</span>");
            gs_tags.entry(git).or_default().entry(e).or_default().push_front(html.clone());

            if tag == "Rule" || tag == "Context" {
                // u = stoul(after " u=\"")
                let upos = ast[last..].find(" u=\"").unwrap() + last + 4;
                let uend = ast[upos..].find('"').unwrap() + upos;
                let u: u32 = ast[upos..uend].parse().unwrap();

                let mut k = crate::profiler::Key { r#type: ET_RULE, id: u };
                if tag == "Context" {
                    k.r#type = ET_CONTEXT;
                }

                if let Some(entry) = profiler.entries.get(&k).copied() {
                    html.clear();
                    html.push_str("<a href=\"");
                    if entry.r#type == ET_RULE {
                        html.push_str("rs/");
                    } else {
                        html.push_str("cs/");
                    }
                    html.push_str(&k.id.to_string());
                    html.push_str(".html\"");
                    if entry.r#type == ET_RULE || rid == 0 {
                        if entry.num_match != 0 {
                            html.push_str(r#" class="entry good"><span class="stats">M:"#);
                        } else {
                            html.push_str(r#" class="entry bad"><span class="stats">M:"#);
                        }
                        html.push_str(&entry.num_match.to_string());
                        html.push_str(", F:");
                        html.push_str(&entry.num_fail.to_string());
                    } else {
                        let ck = (rid, k.id);
                        match profiler.rule_contexts.get(&ck).copied() {
                            Some(second) if second != 0 => {
                                html.push_str(
                                    r#" class="entry context good"><span class="stats">M:"#,
                                );
                                html.push_str(&second.to_string());
                            }
                            _ => {
                                html.push_str(
                                    r#" class="entry context bad"><span class="stats">M:0"#,
                                );
                            }
                        }
                    }
                    html.push_str("</span>");
                    gs_tags.entry(git).or_default().entry(b).or_default().push_back(html.clone());

                    html.clear();
                    html.push_str("</a>");
                    gs_tags.entry(git).or_default().entry(e).or_default().push_front(html.clone());

                    if entry.r#type == ET_RULE {
                        rid = k.id;
                    }
                }
            }

            last += 1;
        }
    }

    // UTF-16 copies of the grammars, to enable extracting snippets from offsets.
    // std::map<size_t, UnicodeString> grammars; (keyed by grammar id == it.second)
    let mut grammars_u16: BTreeMap<usize, Vec<u16>> = BTreeMap::new();
    for (&_fid, &gid) in &profiler.grammars {
        grammars_u16.entry(gid).or_insert_with(|| to_utf16(&sget(gid)));
    }

    // write_grammar lambda, invoked for each grammar.
    for (&fid, &gid) in &profiler.grammars {
        let grammar_u16 = grammars_u16.get(&gid).cloned().unwrap_or_default();
        write_grammar(&profiler, &strings, &gs_tags, fid, &grammar_u16);
    }

    // Helper to write out usage examples.
    let entries: Vec<(u32, crate::profiler::Entry)> = profiler
        .entries
        .iter()
        .map(|(k, e)| (k.id, *e))
        .collect();
    for (id, e) in entries {
        if e.example_window != 0 {
            let g = grammars_u16.get(&(e.grammar as usize)).cloned().unwrap_or_default();
            write_entry(&strings, id, &e, &g);
        }
    }

    // index.html — list of grammars.
    let mut html = String::new();
    for (&fid, &gid) in &profiler.grammars {
        let s_f = xml_encode(&path_filename(&sget(fid)));
        html.push_str(&format!(r#"<li><a href="g{}.html">{}</a></li>"#, gid, s_f));
    }
    let list_body = html;

    let gn = sget(*profiler.grammars.keys().next().unwrap_or(&0));
    let index_html = format!(
        r#"<!DOCTYPE html>
<html>
<head>
	<meta charset="UTF-8">
	<title>{} &laquo; CG-3 Grammar Annotation</title>
	<link rel="stylesheet" href="style.css">
</head>
<body>
<ul>
{}
</ul>
</body>
</html>
"#,
        xml_encode(&path_filename(&gn)),
        list_body
    );
    file_save(std::path::Path::new("index.html"), &index_html);

    file_save(std::path::Path::new("style.css"), STYLE_CSS);

    // C++ main falls off the end (implicit return 0).
    0
}

/// C++ `write_grammar` lambda — emits one `g<gz>.html` for grammar `g` (the
/// fname id). `grammar_u16` is the UTF-16 copy of the grammar source.
fn write_grammar(
    profiler: &Profiler,
    strings: &BTreeMap<usize, String>,
    gs_tags: &BTreeMap<usize, BTreeMap<usize, VecDeque<String>>>,
    g: usize,
    grammar_u16: &[u16],
) {
    let sget = |id: usize| -> String { strings.get(&id).cloned().unwrap_or_default() };

    let gn = sget(g);
    let mut html = format!(
        r#"<!DOCTYPE html>
<html>
<head>
	<meta charset="UTF-8">
	<title>{} annotated</title>
	<link rel="stylesheet" href="style.css">
</head>
<body>
<div id="grammar" class="p-2 pre-wrap">
"#,
        xml_encode(&path_filename(&gn))
    );

    // auto gz = profiler.grammars[g]; auto& tags = gs_tags[gz];
    let gz = *profiler.grammars.get(&g).unwrap_or(&0);
    let empty = BTreeMap::new();
    let tags = gs_tags.get(&gz).unwrap_or(&empty);

    let mut last: usize = 0;
    let mut ln: usize = 1;
    html.push_str(&format!("<span class=\"ln\">{:06}</span>", ln));

    for (&off, tag_list) in tags {
        // tmp = grammar.tempSubStringBetween(last, off); toUTF8; xml_encode.
        let tmp = utf16_slice(grammar_u16, last, off);
        last = off;
        let buf = xml_encode(&tmp);
        for c in buf.chars() {
            html.push(c);
            if c == '\n' {
                ln += 1;
                html.push_str(&format!("<span class=\"ln\">{:06}</span>", ln));
            }
        }
        for tag in tag_list {
            html.push_str(tag);
        }
    }

    html.push_str(
        r#"</div>
</body>
</html>
"#,
    );

    let fname = format!("g{}.html", gz);
    file_save(std::path::Path::new(&fname), &html);
}

/// C++ `write_entry` lambda — emits `rs/<id>.html` or `cs/<id>.html` (a usage
/// example snippet + its example window).
fn write_entry(
    strings: &BTreeMap<usize, String>,
    id: u32,
    e: &crate::profiler::Entry,
    g: &[u16],
) {
    let sget = |id: usize| -> String { strings.get(&id).cloned().unwrap_or_default() };

    let mut html = String::new();
    html.push_str(
        r#"<!DOCTYPE html>
<html>
<head>
	<meta charset="UTF-8">
	<title>Usage example</title>
	<link rel="stylesheet" href="../style.css">
</head>
<body>
<div id="what" class="p-2 pre-wrap">
"#,
    );

    // snip = g.tempSubString(e.b, e.e - e.b); toUTF8; xml_encode.
    let snip = utf16_slice(g, e.b, e.e);
    html.push_str(&xml_encode(&snip));
    html.push_str(
        r#"
</div>
<div id="context" class="p-2 pre-wrap">
"#,
    );
    html.push_str(&xml_encode(&sget(e.example_window)));
    html.push_str(
        r#"
</div>
</body>
</html>
"#,
    );

    let subdir = if e.r#type == ET_CONTEXT { "cs" } else { "rs" };
    let fname = format!("{}/{}.html", subdir, id);
    file_save(std::path::Path::new(&fname), &html);
}

/// The verbatim CSS blob written to `style.css`.
const STYLE_CSS: &str = r#"
html, body {
	background-color: #fff;
	font-family: sans-serif;
}

.p-2 {
	padding: 1ex;
}

a {
	text-decoration: none;
}

.cg-elem {
	position: relative;
}

.cgDelimiters, .cgList, .cgSet, .cgTemplate {
	color: #0000ff;
}

.cgTag {
	color: #008000;
}

.cgSetName, .cgTemplateName {
	color: #800080;
}

.cgSetOp {
	color: #ff00ff;
}

.entry {
	display: inline-block;
	position: relative;
	padding-top: 3ex;
}

#what, .good {
	background-color: #cfc;
}

#context, .context {
	background-color: #ccf;
}

.pre-wrap {
	white-space: pre-wrap;
	font-family: monospace;
	padding-left: 9ex;
}

.bad {
	background-color: #fcc;
}

.stats {
	font-family: sans-serif;
	position: absolute;
	top: 0;
	left: 0;
	white-space: nowrap;
	background-color: #eee;
}

.ln {
	white-space: nowrap;
	margin-right: 1ex;
	margin-left: -9ex;
	background-color: #ddd;
	user-select: none;
	color: #000;
}
"#;
