//! Port of `src/Profiler.hpp` / `src/Profiler.cpp` — the rule/context profiler
//! and its SQLite serialization.
//!
//! Literal, bug-for-bug 1:1 translation (Wave 2). The C++ used the sqlite3 C API
//! directly; this port uses [`rusqlite`] (bundled SQLite) — the four-table schema,
//! declared column order, composite primary keys, PRAGMAs, ordered inserts, and
//! the fixed-10-iteration subsumed-context prune are reproduced verbatim.
//!
//! ## Flagged quirks reproduced
//! * `write` overrides the DB `key` to `0` for whichever interned string carries
//!   the id equal to `grammar_ast` (the grammar AST text is stored under key 0);
//!   `read` does NOT undo this, so that string comes back with id `0` in memory.
//! * The subsumed-context prune runs the DELETE exactly 10 times (a fixed count,
//!   NOT "loop until stable").
//! * The db handle is never explicitly closed (here the [`rusqlite::Connection`]
//!   is dropped at scope end rather than closed via `sqlite3_close` — a documented
//!   deviation; the C++ leaked the handle).
//! * `read` does NOT clear the in-memory maps first — rows are MERGED into whatever
//!   the Profiler already holds.
//! * `addRule` / `addContext` are first-write-wins (`emplace` / `count==0` guard).
//!
//! ## Ordering parity
//! C++ `strings` is a `std::map<std::string, size_t, std::less<>>` (ordered by the
//! string text), `grammars` / `entries` / `rule_contexts` are `std::map`s ordered
//! by their key type. The write path iterates each in ascending key order. The port
//! uses [`std::collections::BTreeMap`] with matching key types so iteration order
//! is identical (byte-lexicographic for the `String` key, numeric for the rest).

use std::collections::BTreeMap;

use rusqlite::{Connection, OpenFlags};

// C++ `enum : uint8_t { ET_RULE = 0, ET_CONTEXT = 1 };`. No spec:def id.
pub const ET_RULE: u8 = 0;
pub const ET_CONTEXT: u8 = 1;

// [spec:cg3:def:profiler.cg3.profiler.key]
/// C++ `struct Profiler::Key { uint8_t type = ET_RULE; uint32_t id = 0; ... }`.
/// The composite `(type, id)` key of the `entries` map.
///
/// The C++ `operator<` orders primarily by `type` then by `id` — exactly the
/// lexicographic order that `#[derive(PartialOrd, Ord)]` produces for
/// `(u8, u32)` fields in declaration order, so the derive faithfully reproduces
/// [`profiler.cg3.profiler.key.operator-fn`]. `Default` gives `type = ET_RULE`
/// (0) and `id = 0`, matching the C++ member initialisers.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Key {
    // [spec:cg3:def:profiler.cg3.profiler.key.operator-fn]
    // [spec:cg3:sem:profiler.cg3.profiler.key.operator-fn]
    // Field order matters: `type` first, then `id`, so the derived `Ord`
    // compares `(type, id)` lexicographically — identical to the C++
    // `operator<`.
    pub r#type: u8,
    pub id: u32,
}

impl Default for Key {
    fn default() -> Self {
        Key { r#type: ET_RULE, id: 0 }
    }
}

// [spec:cg3:def:profiler.cg3.profiler.entry]
/// C++ `struct Profiler::Entry`. The value type of the `entries` map: which
/// grammar the rule/context came from, its `[b, e)` source span, and the running
/// match/fail/example counters.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct Entry {
    pub r#type: u8,
    pub grammar: u32,
    pub b: usize,
    pub e: usize,
    pub num_match: usize,
    pub num_fail: usize,
    pub example_window: usize,
}

impl Default for Entry {
    /// C++ member initialisers: `type = ET_RULE`, everything else `0`.
    fn default() -> Self {
        Entry {
            r#type: ET_RULE,
            grammar: 0,
            b: 0,
            e: 0,
            num_match: 0,
            num_fail: 0,
            example_window: 0,
        }
    }
}

// [spec:cg3:def:profiler.cg3.profiler]
/// The profiler: interned strings, per-grammar links, per-rule/context entries,
/// and per-(rule, context) match counts. `buf` (a C++ `std::stringstream`
/// scratch buffer) is reproduced as a `String` for faithfulness even though the
/// ported methods do not use it.
///
/// * `strings`  — `std::map<std::string, size_t, std::less<>>` → `BTreeMap<String, usize>`
/// * `grammars` — `std::map<size_t, size_t>`                    → `BTreeMap<usize, usize>`
/// * `entries`  — `std::map<Key, Entry>`                        → `BTreeMap<Key, Entry>`
/// * `rule_contexts` — `std::map<std::pair<uint32_t,uint32_t>, size_t>`
///                   → `BTreeMap<(u32, u32), usize>`
#[derive(Default)]
pub struct Profiler {
    pub strings: BTreeMap<String, usize>,
    pub grammars: BTreeMap<usize, usize>,
    pub grammar_ast: usize,
    pub buf: String,
    pub entries: BTreeMap<Key, Entry>,
    pub rule_contexts: BTreeMap<(u32, u32), usize>,
}

// [spec:cg3:def:profiler.cg3.sqlite3-exec-fn]
// [spec:cg3:sem:profiler.cg3.sqlite3-exec-fn]
/// C++ `inline auto sqlite3_exec(sqlite3* db, const char* sql)` — a thin
/// convenience wrapper (namespace `CG3`, shadowing the global) that forwards to
/// the C API `::sqlite3_exec(db, sql, nullptr, nullptr, nullptr)`: run one or more
/// SQL statements with no per-row callback, no callback arg, and no error-message
/// out-pointer, returning the result code. The rusqlite equivalent is
/// [`Connection::execute_batch`] (no callback, no bindings). Exists only so callers
/// can write `sqlite3_exec(db, q)` with two arguments; `Profiler::write` calls this
/// for every PRAGMA/DDL/transaction statement.
#[allow(dead_code)]
fn sqlite3_exec(db: &Connection, sql: &str) -> Result<(), rusqlite::Error> {
    db.execute_batch(sql)
}

impl Profiler {
    // [spec:cg3:def:profiler.cg3.profiler.add-string-fn]
    // [spec:cg3:sem:profiler.cg3.profiler.add-string-fn]
    /// Interns `str`, returning its 1-based id. On first sight computes
    /// `sz = strings.size() + 1` and inserts; ids are assigned 1, 2, 3, … in
    /// insertion order and never reused (nothing is ever erased). The C++
    /// transparent-comparator `find` (no allocation for the `string_view`) is a
    /// pure performance concern; the port takes `&str` and only allocates on the
    /// insert path.
    pub fn add_string(&mut self, str: &str) -> usize {
        if let Some(&id) = self.strings.get(str) {
            return id;
        }
        let sz = self.strings.len() + 1;
        self.strings.insert(str.to_string(), sz);
        sz
    }

    // [spec:cg3:def:profiler.cg3.profiler.add-grammar-fn]
    // [spec:cg3:sem:profiler.cg3.profiler.add-grammar-fn]
    /// Interns `fname` (id `f`) then `grammar` (id `g`), records
    /// `grammars[f] = g` (overwriting any prior value for `f`), and returns
    /// `UI32(g)`. `fname` is interned first, so `f < g` whenever both are newly
    /// seen.
    pub fn add_grammar(&mut self, fname: &str, grammar: &str) -> u32 {
        let f = self.add_string(fname);
        let g = self.add_string(grammar);
        self.grammars.insert(f, g);
        g as u32
    }

    // [spec:cg3:def:profiler.cg3.profiler.add-rule-fn]
    // [spec:cg3:sem:profiler.cg3.profiler.add-rule-fn]
    /// `entries.emplace(Key{ET_RULE, n}, Entry{ET_RULE, g, b, e})` — inserts only
    /// if no entry for the key exists (first write wins); the remaining Entry
    /// fields default to 0.
    pub fn add_rule(&mut self, n: u32, g: u32, b: usize, e: usize) {
        let k = Key { r#type: ET_RULE, id: n };
        // std::map::emplace inserts only when the key is absent.
        self.entries.entry(k).or_insert(Entry {
            r#type: ET_RULE,
            grammar: g,
            b,
            e,
            num_match: 0,
            num_fail: 0,
            example_window: 0,
        });
    }

    // [spec:cg3:def:profiler.cg3.profiler.add-context-fn]
    // [spec:cg3:sem:profiler.cg3.profiler.add-context-fn]
    /// Builds `Key{ET_CONTEXT, c}`; when no entry for it exists yet (`count == 0`),
    /// inserts `Entry{ET_CONTEXT, g, b, e}` (remaining fields 0). Never overwrites
    /// an existing context entry (first write wins).
    pub fn add_context(&mut self, c: u32, g: u32, b: usize, e: usize) {
        let k = Key { r#type: ET_CONTEXT, id: c };
        if !self.entries.contains_key(&k) {
            self.entries.insert(
                k,
                Entry {
                    r#type: ET_CONTEXT,
                    grammar: g,
                    b,
                    e,
                    num_match: 0,
                    num_fail: 0,
                    example_window: 0,
                },
            );
        }
    }

    // [spec:cg3:def:profiler.cg3.profiler.write-fn]
    // [spec:cg3:sem:profiler.cg3.profiler.write-fn]
    /// Serializes the four maps into a fresh SQLite database at `fname`. Deletes
    /// any existing file first (a missing file is fine), applies the MEMORY/
    /// EXCLUSIVE/OFF PRAGMAs, creates the four tables (declared column order is
    /// load-bearing — `read` uses `SELECT *`), opens a transaction, inserts every
    /// row in map order, prunes subsumed contexts (fixed 10 iterations), then
    /// commits.
    ///
    /// rusqlite mapping: the C API `sqlite3_open_v2(RW|CREATE)` →
    /// [`Connection::open`]; each `sqlite3_exec` PRAGMA/DDL →
    /// [`Connection::execute_batch`]; each prepared INSERT →
    /// [`rusqlite::Statement`] with positional `?` params and `execute`. The C++
    /// bound the string bytes with `SQLITE_STATIC` and an explicit length; here
    /// the owned `String` is bound as text (embedded NULs and length are handled
    /// by rusqlite). Any error is surfaced as [`rusqlite::Error`] rather than a
    /// thrown `std::runtime_error` — same failure points, mapped to `Result`.
    pub fn write(&self, fname: &str) -> Result<(), rusqlite::Error> {
        // remove(fname) — value ignored, so a missing file is fine.
        let _ = std::fs::remove_file(fname);

        let db = Connection::open_with_flags(
            fname,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE,
        )?;

        // The ordered init list, each run via a single exec (== sqlite3_exec).
        db.execute_batch("PRAGMA journal_mode = MEMORY")?;
        db.execute_batch("PRAGMA locking_mode = EXCLUSIVE")?;
        db.execute_batch("PRAGMA synchronous = OFF")?;
        db.execute_batch(
            "CREATE TABLE strings (key INTEGER PRIMARY KEY NOT NULL, value TEXT NOT NULL)",
        )?;
        db.execute_batch(
            "CREATE TABLE grammars (fname INTEGER PRIMARY KEY NOT NULL, grammar INTEGER NOT NULL)",
        )?;
        db.execute_batch(
            "CREATE TABLE entries (type INTEGER NOT NULL, id INTEGER NOT NULL, grammar INTEGER NOT NULL, b INTEGER NOT NULL, e INTEGER NOT NULL, num_match INTEGER NOT NULL, num_fail INTEGER NOT NULL, example_window INTEGER NOT NULL, PRIMARY KEY (type, id))",
        )?;
        db.execute_batch(
            "CREATE TABLE rule_contexts (rule INTEGER NOT NULL, context INTEGER NOT NULL, num_match INTEGER NOT NULL, PRIMARY KEY (rule, context))",
        )?;
        db.execute_batch("BEGIN")?;

        // Strings — iterated ascending by string text (BTreeMap order == std::map).
        {
            let mut s =
                db.prepare("INSERT INTO strings (key, value) VALUES (:key, :value)")?;
            for (text, &id) in &self.strings {
                // sz = id, but override to 0 for the grammar_ast-carrying string.
                let sz = if id == self.grammar_ast { 0 } else { id };
                s.execute(rusqlite::params![sz as i64, text])?;
            }
        }

        // Grammars — iterated ascending by fname id.
        {
            let mut s = db
                .prepare("INSERT INTO grammars (fname, grammar) VALUES (:fname, :grammar)")?;
            for (&f, &g) in &self.grammars {
                s.execute(rusqlite::params![f as i64, g as i64])?;
            }
        }

        // Entries — iterated ascending by Key (type, id).
        {
            let mut s = db.prepare(
                "INSERT INTO entries (type, id, grammar, b, e, num_match, num_fail, example_window) VALUES(:type, :id, :grammar, :b, :e, :num_match, :num_fail, :example_window)",
            )?;
            for (k, e) in &self.entries {
                s.execute(rusqlite::params![
                    k.r#type as i64,
                    k.id as i64,
                    e.grammar as i64,
                    e.b as i64,
                    e.e as i64,
                    e.num_match as i64,
                    e.num_fail as i64,
                    e.example_window as i64,
                ])?;
            }
        }

        // Rule->Context hits — iterated ascending by the (rule, context) pair.
        {
            let mut s = db.prepare(
                "INSERT INTO rule_contexts (rule, context, num_match) VALUES (:rule, :context, :num_match)",
            )?;
            for (&(rule, context), &num_match) in &self.rule_contexts {
                s.execute(rusqlite::params![rule as i64, context as i64, num_match as i64])?;
            }
        }

        // Erase contexts that only exist as the linked part of a larger context.
        // FIXED 10 iterations (bug-for-bug: not "loop until stable").
        for _ in 0..10 {
            db.execute_batch(
                "DELETE FROM entries WHERE type = 1 AND id IN (SELECT id FROM entries as et INNER JOIN (SELECT max(b) as b, e FROM entries WHERE type = 1 GROUP BY e HAVING count(b) > 1) as jt ON (et.b = jt.b AND et.e = jt.e))",
            )?;
        }

        db.execute_batch("COMMIT")?;
        // db handle: C++ never closes it. Here `db` drops at scope end.
        Ok(())
    }

    // [spec:cg3:def:profiler.cg3.profiler.read-fn]
    // [spec:cg3:sem:profiler.cg3.profiler.read-fn]
    /// Loads a previously written profile database at `fname` into this Profiler's
    /// maps. The maps are NOT cleared first — rows are MERGED into whatever the
    /// Profiler already holds. Each of the four tables is read with `SELECT *`
    /// (so column order is exactly the declared order) row by row. No error
    /// checking is done on the rows themselves.
    ///
    /// QUIRK: the strings table restores the key verbatim, so the string whose id
    /// equalled `grammar_ast` at write time (stored under key 0) comes back with
    /// id 0 in memory — read does NOT undo the `write` override.
    ///
    /// rusqlite mapping: `sqlite3_open_v2(READONLY)` →
    /// [`Connection::open_with_flags`] with `SQLITE_OPEN_READ_ONLY`; each prepared
    /// `SELECT *` → [`rusqlite::Statement::query`] stepped via the returned
    /// [`rusqlite::Rows`]; `sqlite3_column_int64`/`_text` → `row.get`. The db
    /// handle is left to drop at scope end (C++ never closed it).
    pub fn read(&mut self, fname: &str) -> Result<(), rusqlite::Error> {
        let db = Connection::open_with_flags(fname, OpenFlags::SQLITE_OPEN_READ_ONLY)?;

        // Strings (columns key, value): strings[value] = key.
        {
            let mut s = db.prepare("SELECT * FROM strings")?;
            let mut rows = s.query([])?;
            while let Some(row) = rows.next()? {
                let sz: i64 = row.get(0)?;
                let tmp: String = row.get(1)?;
                self.strings.insert(tmp, sz as usize);
            }
        }

        // Grammars (columns fname, grammar): grammars[f] = g.
        {
            let mut s = db.prepare("SELECT * FROM grammars")?;
            let mut rows = s.query([])?;
            while let Some(row) = rows.next()? {
                let f: i64 = row.get(0)?;
                let g: i64 = row.get(1)?;
                self.grammars.insert(f as usize, g as usize);
            }
        }

        // Entries (columns type, id, grammar, b, e, num_match, num_fail,
        // example_window): entries[Key{type,id}] default-inserted then assigned.
        {
            let mut s = db.prepare("SELECT * FROM entries")?;
            let mut rows = s.query([])?;
            while let Some(row) = rows.next()? {
                let type_: u8 = row.get::<_, i64>(0)? as u8;
                let id: u32 = row.get::<_, i64>(1)? as u32;
                let k = Key { r#type: type_, id };
                let e = self.entries.entry(k).or_default();
                e.r#type = type_;
                e.grammar = row.get::<_, i64>(2)? as u32;
                e.b = row.get::<_, i64>(3)? as usize;
                e.e = row.get::<_, i64>(4)? as usize;
                e.num_match = row.get::<_, i64>(5)? as usize;
                e.num_fail = row.get::<_, i64>(6)? as usize;
                e.example_window = row.get::<_, i64>(7)? as usize;
            }
        }

        // Rule->Context hits (columns rule, context, num_match).
        {
            let mut s = db.prepare("SELECT * FROM rule_contexts")?;
            let mut rows = s.query([])?;
            while let Some(row) = rows.next()? {
                let r: u32 = row.get::<_, i64>(0)? as u32;
                let c: u32 = row.get::<_, i64>(1)? as u32;
                let num_match: usize = row.get::<_, i64>(2)? as usize;
                self.rule_contexts.insert((r, c), num_match);
            }
        }

        Ok(())
    }
}
