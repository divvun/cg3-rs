# src/Profiler.cpp, src/Profiler.hpp

> [spec:cg3:def:profiler.cg3.profiler]
> struct Profiler {
>   std::map<std::string, size_t, std::less<>> strings;
>   std::map<size_t, size_t> grammars;
>   size_t grammar_ast = 0;
>   std::stringstream buf;
>   struct Key { uint8_t type = ET_RULE; uint32_t id = 0; bool operator<(const Key& o) const { if (type == o.type) { return id < o.id; } return type < o.type; } };
>   struct Entry { uint8_t type = ET_RULE; uint32_t grammar = 0; size_t b = 0; size_t e = 0; size_t num_match = 0; size_t num_fail = 0; size_t example_window = 0; };
>   std::map<Key, Entry> entries;
>   std::map<std::pair<uint32_t, uint32_t>, size_t> rule_contexts;
> }

> [spec:cg3:def:profiler.cg3.profiler.add-context-fn]
> void addContext(uint32_t c, uint32_t g, size_t b, size_t e)

> [spec:cg3:sem:profiler.cg3.profiler.add-context-fn]
> Builds a `Key k` with `type = ET_CONTEXT` (the enum value 1) and
> `id = c`. Looks up `k` in the `entries` map: if `entries.count(k) == 0`
> (no entry with that (type,id) exists yet), inserts a new entry
> `entries[k] = Entry{ type = ET_CONTEXT, grammar = g, b = b, e = e }`
> where the remaining Entry fields `num_match`, `num_fail`,
> `example_window` take their default value 0. If an entry for `k`
> already exists, the function does nothing — it never overwrites an
> existing context entry (first write wins). No return value; the only
> side effect is the possible insertion into `entries`.

> [spec:cg3:def:profiler.cg3.profiler.add-grammar-fn]
> uint32_t addGrammar(std::string_view fname, std::string_view grammar)

> [spec:cg3:sem:profiler.cg3.profiler.add-grammar-fn]
> Interns two strings and links them. Calls `addString(fname)` to get a
> `size_t` id `f` for the grammar's file name, then `addString(grammar)`
> to get a `size_t` id `g` for the grammar's textual content. Records the
> mapping `grammars[f] = g` (map assignment, overwriting any prior value
> for key `f`). Returns `UI32(g)` — the content-string id `g` truncated
> to `uint32_t`. Because `addString` is called for `fname` first and
> `grammar` second, `f` is assigned before `g`, so `f < g` whenever both
> strings are newly seen.

> [spec:cg3:def:profiler.cg3.profiler.add-rule-fn]
> void addRule(uint32_t n, uint32_t g, size_t b, size_t e)

> [spec:cg3:sem:profiler.cg3.profiler.add-rule-fn]
> Builds a `Key k` with `type = ET_RULE` (enum value 0) and `id = n`.
> Calls `entries.emplace(k, Entry{ ET_RULE, g, b, e })`, i.e. inserts an
> entry keyed by `k` with `type = ET_RULE`, `grammar = g`, `b = b`,
> `e = e`, and the remaining fields (`num_match`, `num_fail`,
> `example_window`) left at their default 0. `emplace` inserts only if no
> entry for `k` already exists; if one does, the existing entry is left
> unchanged (first write wins). No return value.

> [spec:cg3:def:profiler.cg3.profiler.add-string-fn]
> size_t addString(std::string_view str)

> [spec:cg3:sem:profiler.cg3.profiler.add-string-fn]
> Interns a string and returns its 1-based id. `strings` is a
> `std::map<std::string, size_t, std::less<>>` (transparent comparator,
> so `str` — a `string_view` — can be looked up without allocating).
> Does `strings.find(str)`: if found, returns the stored id
> (`it->second`) unchanged. Otherwise computes `sz = strings.size() + 1`
> (the current entry count plus one), inserts `strings.emplace(
> std::string(str), sz)` (copying the view into an owned string keyed to
> `sz`), and returns `sz`. Ids are therefore assigned 1, 2, 3, … in order
> of first insertion and never reused or renumbered (the map is never
> erased from). Note the id is derived from the map size at insertion
> time, so it is dense and monotonic only because nothing is ever
> removed.

> [spec:cg3:def:profiler.cg3.profiler.entry]
> struct Entry {
>   uint8_t type = ET_RULE;
>   uint32_t grammar = 0;
>   size_t b = 0;
>   size_t e = 0;
>   size_t num_match = 0;
>   size_t num_fail = 0;
>   size_t example_window = 0;
> }

> [spec:cg3:def:profiler.cg3.profiler.key]
> struct Key {
>   uint8_t type = ET_RULE;
>   uint32_t id = 0;
> }

> [spec:cg3:def:profiler.cg3.profiler.key.operator-fn]
> bool operator<(const Key& o) const

> [spec:cg3:sem:profiler.cg3.profiler.key.operator-fn]
> Strict-weak `<` ordering used to key the `entries` map. Compares the
> two keys lexicographically by `(type, id)`: if `type == o.type`, returns
> `id < o.id`; otherwise returns `type < o.type`. In other words, orders
> primarily by the 1-byte `type` (ET_RULE = 0 sorting before
> ET_CONTEXT = 1) and secondarily by the 32-bit `id`. `const` and side
> effect free.

> [spec:cg3:def:profiler.cg3.profiler.read-fn]
> void Profiler::read(const char* fname)

> [spec:cg3:sem:profiler.cg3.profiler.read-fn]
> Loads a previously written profile SQLite database at path `fname` into
> this Profiler's in-memory maps. (When compiled with DISABLE_PROFILING
> this function instead just `throw`s std::runtime_error("Profiling
> disabled"); the behaviour below is the normal SQLite build.)
> Steps: call `sqlite3_initialize()`; if it does not return SQLITE_OK,
> throw std::runtime_error("sqlite3_initialize() errored"). Open the
> database read-only via `sqlite3_open_v2(fname, &db, SQLITE_OPEN_READONLY,
> nullptr)`; on non-OK throw a runtime_error whose message is
> `concat("sqlite3_open_v2() error: ", sqlite3_errmsg(db))`.
> The maps are NOT cleared first — rows read are merged into whatever the
> Profiler already holds. Each of the four tables is read with a
> `SELECT * FROM <table>` prepared statement (so column order is exactly
> the table's declared column order), stepped row by row while
> `sqlite3_step` returns SQLITE_ROW, then finalized. A failure to prepare
> any statement throws a runtime_error("sqlite3 error preparing select
> from <table> table: " + errmsg). No error checking is done on the rows
> themselves.
> Strings table (columns key, value): for each row, `sz = UIZ(column_int64(
> 0))` (the key cast to size_t), `tmp = column_text(1)` (the value as a
> C string), then `strings[std::move(tmp)] = sz`. Note this restores the
> key verbatim — the write path stored the string whose id equals
> `grammar_ast` under key 0 (see write-fn), and read does NOT undo that, so
> that string comes back with id 0 in memory.
> Grammars table (columns fname, grammar): `f = UIZ(column_int64(0))`,
> `g = UIZ(column_int64(1))`, then `grammars[f] = g`.
> Entries table (columns type, id, grammar, b, e, num_match, num_fail,
> example_window): `type = UI8(column_int64(0))`,
> `id = UI32(column_int64(1))`, form `Key k{type, id}`, take a reference to
> `entries[k]` (default-inserting if absent), and assign `e.type = type`,
> `e.grammar = UI32(column_int64(2))`, `e.b = UIZ(column_int64(3))`,
> `e.e = UIZ(column_int64(4))`, `e.num_match = UIZ(column_int64(5))`,
> `e.num_fail = UIZ(column_int64(6))`,
> `e.example_window = UIZ(column_int64(7))`.
> Rule_contexts table (columns rule, context, num_match):
> `r = UI32(column_int64(0))`, `c = UI32(column_int64(1))`, then
> `rule_contexts[std::pair(r, c)] = UIZ(column_int64(2))`.
> No return value; leaves `db` open (never closed) after the last
> finalize.

> [spec:cg3:def:profiler.cg3.profiler.write-fn]
> void Profiler::write(const char* fname)

> [spec:cg3:sem:profiler.cg3.profiler.write-fn]
> Serializes this Profiler's four maps into a fresh SQLite database at
> path `fname`. (Under DISABLE_PROFILING the whole body is replaced by
> `throw std::runtime_error("Profiling disabled");`; the following is the
> normal SQLite build.)
> 1. `sqlite3_initialize()`; if not SQLITE_OK, throw runtime_error(
> "sqlite3_initialize() errored").
> 2. `remove(fname)` — delete any existing file at that path first (return
> value ignored, so a missing file is fine).
> 3. `sqlite3_open_v2(fname, &db, SQLITE_OPEN_READWRITE |
> SQLITE_OPEN_CREATE, nullptr)`; on non-OK throw runtime_error(concat(
> "sqlite3_open_v2() error: ", sqlite3_errmsg(db))).
> 4. Execute this ordered list of statements, each via the local
> `sqlite3_exec(db, sql)` wrapper; on any non-OK result throw
> runtime_error(concat("sqlite3 error while initializing database: ",
> errmsg)). The statements, in order, are: `PRAGMA journal_mode = MEMORY`;
> `PRAGMA locking_mode = EXCLUSIVE`; `PRAGMA synchronous = OFF`; then the
> four CREATE TABLE statements; then `BEGIN` (opens a transaction). The
> schema (declared column order matters, since read uses SELECT *):
> `strings (key INTEGER PRIMARY KEY NOT NULL, value TEXT NOT NULL)`;
> `grammars (fname INTEGER PRIMARY KEY NOT NULL, grammar INTEGER NOT NULL)`;
> `entries (type INTEGER NOT NULL, id INTEGER NOT NULL, grammar INTEGER NOT
> NULL, b INTEGER NOT NULL, e INTEGER NOT NULL, num_match INTEGER NOT NULL,
> num_fail INTEGER NOT NULL, example_window INTEGER NOT NULL, PRIMARY KEY
> (type, id))`; `rule_contexts (rule INTEGER NOT NULL, context INTEGER NOT
> NULL, num_match INTEGER NOT NULL, PRIMARY KEY (rule, context))`.
> 5. Strings: prepare `INSERT INTO strings (key, value) VALUES (:key,
> :value)`. Iterate `strings` (a std::map, so ascending by the string
> value/key of the map — i.e. by string text). For each (string, id): reset
> the statement; set `sz = id`, but if `sz == grammar_ast` then override
> `sz = 0` (so whichever interned string carries the grammar_ast id is
> stored under DB key 0); bind `sz` as int64 to param 1 (key); bind the
> string bytes as text to param 2 (value) with length `SI32(size)` and
> SQLITE_STATIC; step (must return SQLITE_DONE). Any bind/step failure
> throws a runtime_error with a message naming the failed operation. Then
> finalize.
> 6. Grammars: prepare `INSERT INTO grammars (fname, grammar) VALUES
> (:fname, :grammar)`. For each (f, g) in `grammars` (map ascending by f):
> reset; bind int64 f -> param 1, g -> param 2; step; finalize.
> 7. Entries: prepare `INSERT INTO entries (type, id, grammar, b, e,
> num_match, num_fail, example_window) VALUES(:type, :id, :grammar, :b, :e,
> :num_match, :num_fail, :example_window)`. For each (Key, Entry) in
> `entries` (ordered by Key::operator<, i.e. by (type, id)): reset; bind
> int64 params 1..8 = key.type, key.id, entry.grammar, entry.b, entry.e,
> entry.num_match, entry.num_fail, entry.example_window; step; finalize.
> 8. Rule_contexts: prepare `INSERT INTO rule_contexts (rule, context,
> num_match) VALUES (:rule, :context, :num_match)`. For each ((rule,
> context), num_match) in `rule_contexts` (map ascending by the pair):
> reset; bind int64 params 1..3 = first.first (rule), first.second
> (context), value (num_match); step; finalize.
> 9. Prune subsumed contexts: run the following DELETE statement exactly
> ten times in a `for (i = 0; i < 10; ++i)` loop (a fixed iteration count,
> not "loop until stable"), throwing runtime_error(concat("sqlite3 error
> while deleting overlapping contexts: ", errmsg)) on any non-OK: `DELETE
> FROM entries WHERE type = 1 AND id IN (SELECT id FROM entries as et INNER
> JOIN (SELECT max(b) as b, e FROM entries WHERE type = 1 GROUP BY e HAVING
> count(b) > 1) as jt ON (et.b = jt.b AND et.e = jt.e))`. Meaning: among
> context rows (type = 1) grouped by end position `e`, for each group that
> has more than one row it takes `max(b)` (the context that begins latest,
> i.e. the innermost/shortest sharing that end), joins entries on
> `b = max(b) AND e`, and deletes those context ids. Repeating up to ten
> times peels off, per end position, successively the current
> largest-`b` context. Comment explains these are contexts that only exist
> as the linked part of a larger context and are "currently irrelevant".
> 10. Execute `COMMIT`; on non-OK throw runtime_error(concat("sqlite3
> error while committing: ", errmsg)).
> No return value; the db handle is never explicitly closed.

> [spec:cg3:def:profiler.cg3.sqlite3-exec-fn]
> inline auto sqlite3_exec(sqlite3* db, const char* sql)

> [spec:cg3:sem:profiler.cg3.sqlite3-exec-fn]
> A thin convenience wrapper (in namespace CG3, shadowing the global) that
> forwards to the C API `::sqlite3_exec(db, sql, nullptr, nullptr,
> nullptr)` — i.e. runs one or more SQL statements in `sql` against `db`
> with no per-row callback, no callback argument, and no error-message-out
> pointer — and returns its `int` result code (SQLITE_OK on success). It
> exists only so callers can write `sqlite3_exec(db, q)` with two
> arguments instead of five.

