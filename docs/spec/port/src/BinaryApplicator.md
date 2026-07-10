# src/BinaryApplicator.cpp, src/BinaryApplicator.hpp

> [spec:cg3:def:binary-applicator.cg3.binary-applicator]
> class BinaryApplicator : public virtual GrammarApplicator {
>   bool header_done = false;
>   UString text;
> }

> [spec:cg3:def:binary-applicator.cg3.binary-applicator.binary-applicator-fn]
> BinaryApplicator::BinaryApplicator(std::ostream& ux_err)

> [spec:cg3:sem:binary-applicator.cg3.binary-applicator.binary-applicator-fn]
> Constructor. Delegates to the base `GrammarApplicator(ux_err)` constructor with
> an empty body. The subclass members take their in-class defaults: `header_done
> = false` and `text` = empty UString. No other side effects.

> [spec:cg3:def:binary-applicator.cg3.binary-applicator.print-plain-text-line-fn]
> void BinaryApplicator::printPlainTextLine(UStringView line, std::ostream& output)

> [spec:cg3:sem:binary-applicator.cg3.binary-applicator.print-plain-text-line-fn]
> Writes a text packet. If `header_done` is false, first write `"CGBF"` +
> `writeLE(output, CG3_BINARY_STREAM)` (u32 LE) and set `header_done = true`. Then
> write one byte `UI8(BFP_TEXT)` (= 3), followed by the line via
> `writeUTF8_LE(output, line)`, which emits `[u16 LE UTF-8 byte-length][UTF-8
> bytes]`. No flush is performed here.

> [spec:cg3:def:binary-applicator.cg3.binary-applicator.print-single-window-fn]
> void BinaryApplicator::printSingleWindow(SingleWindow* window, std::ostream& output, bool profiling)

> [spec:cg3:sem:binary-applicator.cg3.binary-applicator.print-single-window-fn]
> Writes one window packet (the exact inverse of `readWindow`). The `profiling`
> argument is ignored. All multi-byte integers are LITTLE-ENDIAN.
> Stream header: if `header_done` is false, write the 4 bytes `"CGBF"` then
> `writeLE(output, CG3_BINARY_STREAM)` (a `uint32_t` LE = 1), and set `header_done
> = true`. Then write one byte `UI8(BFP_WINDOW)` (= 1) as the packet type.
> The body is built into two scratch `std::string` buffers (`header_buffer` and
> `cohort_buffer`) using a shared per-window tag table: `tags_to_write` (vector)
> and `tag_index` (map from `Tag*` to its 0-based u16 index). Helpers:
> WRITE_U16_INTO / WRITE_U32_INTO append 2/4 little-endian bytes; WRITE_TAG_INTO
> registers a tag into the table if new and appends its u16 index; WRITE_STR_INTO
> UTF-8-encodes a UString (scratch capacity `size*4`), then appends `[u16 LE
> byte-length][UTF-8 bytes]`.
> Variables: iterate `window->variables_output`; for each `var`, `++var_count`,
> `key = grammar->single_tags[var]`. If `var` is in `variables_set`: if its value
> is not `grammar->tag_any`, append byte `BFV_SETVAR` (1) then WRITE_TAG key then
> WRITE_TAG the value tag; else append byte `BFV_SETVAR_ANY` (2), WRITE_TAG key,
> then WRITE_U16 0 (placeholder value slot). If `var` is not in `variables_set`
> (i.e. removed), append byte `BFV_REMVAR` (3), WRITE_TAG key, WRITE_U16 0. This
> all goes into `var_buffer`.
> Reflow removed-cohort text: iterate `window->all_cohorts` by index `i`; for each
> cohort with `local_number == 0` or `CT_REMOVED` that has non-empty `text`, scan
> backwards `j = i .. 1` and, for the FIRST prior cohort that is NOT removed,
> append this cohort's text to it and clear this cohort's text. (Quirk: the inner
> loop has NO break, so after clearing, later iterations append the now-empty
> string — a no-op; net effect is "move to nearest prior non-removed cohort".) If
> no prior non-removed cohort exists, append the (still non-empty) text to
> `window->text` instead and clear it.
> Cohorts: iterate `window->all_cohorts`, skipping any with `local_number == 0` or
> `CT_REMOVED`. For each kept cohort: `unignoreAll()`, `++cohort_count`. Build a
> cohort record into `cohort_buffer`: WRITE_U16 flags (`BFC_RELATED` bit if
> `CT_RELATED`). WRITE_TAG wordform. Static tags: if `cohort->wread`, emit each of
> its `tags_list` (excluding the wordform hash) as tag indices into a temp buffer,
> then WRITE_U16 the count, then append the temp buffer; else WRITE_U16 0.
> Dependency: WRITE_U32 `cohort->global_number` (this is the "self" slot read back
> as `dep_self`); then for the parent slot: if `dep_parent` is 0 or
> `DEP_NO_PARENT`, WRITE_U32 `dep_parent` as-is; else if `gWindow->cohort_map`
> contains `dep_parent`, look up that cohort `pr` and WRITE_U32 0 when
> `pr->local_number == 0` else `pr->global_number`; else WRITE_U32 `DEP_NO_PARENT`.
> Relations: for each `(name_hash, targets)` in `cohort->relations`, look up the
> tag and for each target `++rel_count`, WRITE_TAG the relation-name tag, WRITE_U32
> the target; then WRITE_U16 `rel_count` and append the relation buffer.
> WRITE_STR `cohort->text`, WRITE_STR `cohort->wblank`.
> Readings: sort `cohort->readings` by `Reading::cmp_number`. For each top reading
> whose `noprint` is false, walk its subreading chain via `->next`: for each link,
> `++reading_count`; flags = `BFR_SUBREADING` (bit 0) if this is not the top
> reading else 0; WRITE_U16 flags; WRITE_TAG `single_tags[reading->baseform]`;
> then for each tag in `reading->tags_list`, skip it if it equals the baseform or
> the parent wordform hash, skip if `T_DEPENDENCY | T_RELATION`, apply
> `unique_tags` dedup, else WRITE_TAG it and count; WRITE_U16 that tag count then
> append the tag bytes. (Deleted readings are NOT written by this function; only
> `cohort->readings` are traversed.) WRITE_U16 `reading_count`, append the reading
> buffer.
> Header buffer (assembled AFTER the cohort buffer so the tag table is complete):
> WRITE_U16 window flags (`BFW_DEP_SPAN` bit if `dep_has_spanned`); WRITE_U16
> `tags_to_write.size()` then WRITE_STR each tag's text in table order; WRITE_U16
> `var_count` then append `var_buffer`; WRITE_STR `window->text`; WRITE_STR
> `window->text_post`; WRITE_U16 `cohort_count`.
> Emit: `total_size = header_buffer.size() + cohort_buffer.size()`; write
> `writeLE(output, UI32(total_size))`, then write `header_buffer` bytes, then
> `cohort_buffer` bytes. If `window->flush_after`, emit
> `printStreamCommand(STR_CMD_FLUSH, output)`. Flush `output`.

> [spec:cg3:def:binary-applicator.cg3.binary-applicator.print-stream-command-fn]
> void BinaryApplicator::printStreamCommand(UStringView cmd, std::ostream& output)

> [spec:cg3:sem:binary-applicator.cg3.binary-applicator.print-stream-command-fn]
> Writes a command packet. If `header_done` is false, first write `"CGBF"` +
> `writeLE(output, CG3_BINARY_STREAM)` (u32 LE) and set `header_done = true`. Then
> write one byte `UI8(BFP_COMMAND)` (= 2). Then map the textual command `cmd` to a
> single command byte and write it via `writeLE(output, UI8(...))`: `STR_CMD_FLUSH`
> -> `BFC_FLUSH` (1), `STR_CMD_EXIT` -> `BFC_EXIT` (2), `STR_CMD_IGNORE` ->
> `BFC_IGNORE` (3), `STR_CMD_RESUME` -> `BFC_RESUME` (4). If `cmd` matches none of
> these, only the type byte is written and no command byte follows (malformed
> packet). No flush is performed here.

> [spec:cg3:def:binary-applicator.cg3.binary-applicator.read-command-fn]
> void BinaryApplicator::readCommand(void*& payload)

> [spec:cg3:sem:binary-applicator.cg3.binary-applicator.read-command-fn]
> Reads a command packet's payload. Reads exactly one byte via
> `readLE<uint8_t>(*ux_stdin)` into `cmd` and stores that byte value inside the
> `void* payload` pointer itself (not pointed-to memory): `payload =
> reinterpret_cast<void*>(static_cast<uintptr_t>(cmd))`. The caller later recovers
> the command by casting the pointer back to `uint8_t`. So on the wire a command
> packet is just the single command byte following the `BFP_COMMAND` type byte.

> [spec:cg3:def:binary-applicator.cg3.binary-applicator.read-packet-fn]
> BinaryPacket BinaryApplicator::readPacket()

> [spec:cg3:sem:binary-applicator.cg3.binary-applicator.read-packet-fn]
> Reads one wire packet from `*ux_stdin` and returns a `BinaryPacket`. Steps:
> default-construct `packet` (`type = BFP_INVALID`, `payload = nullptr`). Read one
> byte into `packet.type` via `readLE(*ux_stdin, packet.type)` (a `uint8_t`; the
> little-endian swap is a no-op for one byte). Dispatch on the type: if
> `BFP_WINDOW` (1) call `readWindow(packet.payload)`; else if `BFP_COMMAND` (2)
> call `readCommand(packet.payload)`. Then, in a SEPARATE `if` (not chained),
> if the type is `BFP_TEXT` (3) call `readText(packet.payload)`. Return `packet`.
> An unknown/`BFP_INVALID` type leaves `payload` null. Note: reads at EOF leave
> `type` unchanged/whatever `readLE` produced; the caller loops on `!input.eof()`.

> [spec:cg3:def:binary-applicator.cg3.binary-applicator.read-text-fn]
> void BinaryApplicator::readText(void*& payload)

> [spec:cg3:sem:binary-applicator.cg3.binary-applicator.read-text-fn]
> Reads a text packet's payload into the applicator's reusable `text` member via
> `readUTF8_LE(*ux_stdin, text)`: this reads a little-endian `uint16_t` byte
> length, then that many UTF-8 bytes, and decodes them into the `UString text`
> (overwriting any previous contents). Sets `payload = &text` (points at the
> member). So on the wire a text packet is `[u16 LE byte-length][UTF-8 bytes]`
> following the `BFP_TEXT` type byte.

> [spec:cg3:def:binary-applicator.cg3.binary-applicator.read-window-fn]
> void BinaryApplicator::readWindow(void*& payload)

> [spec:cg3:sem:binary-applicator.cg3.binary-applicator.read-window-fn]
> Reads one window packet body into a new `SingleWindow`. All multi-byte integers
> are LITTLE-ENDIAN. Steps: read a `uint32_t cs` (LE) = the byte length of the
> rest of the window body. If the stream is now at EOF, set `payload = nullptr`
> and return (no window). Otherwise allocate a new SingleWindow via
> `gWindow->allocAppendSingleWindow()` and `initEmptySingleWindow`. Read exactly
> `cs` bytes into a scratch `std::string buf` and parse it with a cursor
> `pos = 0` using these primitives: READ_U16 = little-endian `uint16_t` at `pos`
> (advance 2); READ_U32 = little-endian `uint32_t` (advance 4); READ_STR = read a
> `uint16_t tl` (LE) then decode `tl` UTF-8 bytes at `pos` into a UString (via
> `u_strFromUTF8`, capacity `tl` UChars which always suffices) and advance `pos`
> by `tl`. So every string field on the wire is `[u16 LE byte-length][UTF-8
> bytes]`.
> Layout parsed, in order:
> 1. Window flags: `flags = READ_U16()`. If `flags & BFW_DEP_SPAN` (bit 0) set
>    `dep_has_spanned = true`.
> 2. Tag table: `tag_count = READ_U16()`, then `tag_count` strings. Each is
>    READ_STR into `tg`, appended to a local `window_tags` vector as
>    `addTag(tg)`. If `tg[0] == grammar->mapping_prefix` OR the tag object
>    already, force `T_MAPPING` on it, else clear `T_MAPPING`. All later tag
>    references in this window are 0-based indices into `window_tags`.
> 3. Variables: `var_count = READ_U16()`, then that many entries. Each entry is
>    `[1 byte mode][u16 tag-index key][u16 tag-index value]` (the mode byte is
>    read as `buf[pos]` then `++pos`; the two indices via READ_U16). Let `hash1 =
>    window_tags[key]->hash`. If `mode == BFV_SETVAR` (1):
>    `variables_set[hash1] = window_tags[value]->hash`, erase `hash1` from
>    `variables_rem`, insert into `variables_output`. If `mode == BFV_SETVAR_ANY`
>    (2): `variables_set[hash1] = grammar->tag_any` (the value index is read but
>    ignored), erase from rem, insert into output. If `mode == BFV_REMVAR` (3):
>    erase `hash1` from `variables_set`, insert into `variables_rem`, insert into
>    `variables_output`.
> 4. `READ_STR` into `cSWindow->text`, then `READ_STR` into `cSWindow->text_post`.
> 5. Cohorts: `cohort_count = READ_U16()`, then that many cohort records. For each:
>    allocate `cCohort = alloc_cohort(cSWindow)`, `global_number =
>    gWindow->cohort_counter++`, `++numCohorts`. Read cohort `flags = READ_U16()`;
>    if `flags & BFC_RELATED` (bit 0) set `cCohort->type |= CT_RELATED` and
>    `has_relations = true`. Read `tag = READ_U16()`; `cCohort->wordform =
>    window_tags[tag]`. Read static-tag count `tag_count = READ_U16()`; if
>    non-zero, allocate `cCohort->wread`, add the wordform to it, then loop
>    `tag_count` times reading a u16 index and `addTagToReading(*wread,
>    window_tags[tag], rehash=(tn+1==tag_count))` (only the last add rehashes the
>    reading). Read `dep_self = READ_U32()`, `dep_parent = READ_U32()`; set
>    `gWindow->relation_map[dep_self] = global_number`; if `dep_parent !=
>    DEP_NO_PARENT` set `has_dep = true`. Read relation count `rel_count =
>    READ_U16()`; each relation is `[u16 tag-index][u32 head]`:
>    `cCohort->relations_input[window_tags[tag]->hash].insert(head)`. If
>    `rel_count > 0` set `has_relations = true`, re-set `relation_map[dep_self] =
>    global_number`, and `cCohort->type |= CT_RELATED`. `READ_STR` into
>    `cCohort->text`, then `READ_STR` into `cCohort->wblank`. Read reading count
>    `reading_count = READ_U16()`; if zero, `initEmptyCohort(*cCohort)`. Keep a
>    `prev` reading pointer (initially null). For each reading: allocate
>    `cReading = alloc_reading(cCohort)`, add the wordform tag. Read reading
>    `flags = READ_U16()`. Read baseform index and `addTagToReading(*cReading,
>    window_tags[tag])`. Read reading tag count `tag_count = READ_U16()`; for each
>    tag index, if `window_tags[tag]->type & T_MAPPING` push onto a `mappings`
>    list, else `addTagToReading`; after the loop, if `mappings` is non-empty call
>    `splitMappings(mappings, *cCohort, *cReading, true)`. Placement: if `prev` is
>    set and `flags & BFR_SUBREADING` (bit 0) chain `prev->next = cReading` (a
>    subreading); else if `flags & BFR_DELETED` (bit 1) push to
>    `cCohort->deleted`; else `cCohort->appendReading(cReading)`. Set `prev =
>    cReading`, `++numReadings`. After all readings of the LAST cohort
>    (`cn+1 == cohort_count`), for each reading in `cCohort->readings` that does
>    not already contain `endtag`, `addTagToReading(*iter, endtag)`. Then
>    `insert_if_exists(cCohort->possible_sets, grammar->sets_any)` and
>    `cSWindow->appendCohort(cCohort)`.
> Finally set `payload = cSWindow`. No bounds checking is done on `window_tags`
> indices — a malformed index is undefined behavior.

> [spec:cg3:def:binary-applicator.cg3.binary-applicator.run-grammar-on-text-fn]
> void BinaryApplicator::runGrammarOnText(std::istream& input, std::ostream& output)

> [spec:cg3:sem:binary-applicator.cg3.binary-applicator.run-grammar-on-text-fn]
> Drives the binary STREAM protocol: reads an 8-byte header, then a sequence of
> packets (window / command / text), running the grammar over windows and printing
> results.
> Setup: store `&input` in `ux_stdin`, `&output` in `ux_stdout`. If
> `!input.good()` print "Error: Input is null..." and `CG3Quit(1)`; if
> `input.eof()` print "Error: Input is empty..." and quit; if `!output` print
> "Error: Output is null..." and quit; if `!grammar` print "Error: No grammar
> provided..." and quit.
> Header: read exactly 8 bytes into a `std::string header`; on read failure print
> "Error: Could not read stream header!" and quit. Check the first 4 bytes are the
> magic `"CGBF"` via `is_cg3bsf(header)` (note: the STREAM magic is `CGBF`, which
> is distinct from the `.cg3b` grammar magic `CG3B`); if not, print "Error: Stream
> does not start with magic bytes..." and quit. Read the version as
> `reinterpret_cast<uint32_t*>(&header[4])[0]` — i.e. the 4 version bytes in
> NATIVE byte order, NOT byte-swapped. If `version != CG3_BINARY_STREAM` (= 1)
> print "Error: Stream is version %u but this reader only knows version %u!" and
> quit. (Quirk/latent bug: the writer emits the version via `writeLE` in
> little-endian, but the reader interprets it natively, so on a big-endian host
> the version check would spuriously fail.)
> Call `index()`. Compute `resetAfter = (num_windows + 4) * 2 + 1`. Set
> `gWindow->window_span = num_windows`.
> Define a local `flush(flush_after=false)` lambda: `backSWindow = gWindow->back()`;
> if non-null set `backSWindow->flush_after = flush_after`. Then while
> `gWindow->next` is non-empty, `shuffleWindowsDown` + `runGrammarOnWindow`; then
> `shuffleWindowsDown()` once more; then while `gWindow->previous` is non-empty pop
> the front, `printSingleWindow(tmp, output)`, `free_swindow(tmp)`, erase it.
> Returns `backSWindow`.
> Main loop: `while (!input.eof())`, read `packet = readPacket()`. Dispatch on
> `packet.type`:
> - `BFP_WINDOW` (1): `++numWindows`; if `gWindow->next.size() > num_windows`
>   then `shuffleWindowsDown`, `runGrammarOnWindow`, and if `numWindows %
>   resetAfter == 0` call `resetIndexes()`. (The parsed window itself was already
>   appended to `gWindow` inside `readWindow`; its payload pointer is unused here.)
> - `BFP_COMMAND` (2): recover the command byte by casting `packet.payload` back
>   to `uint8_t`. `BFC_FLUSH` (1): call `flush(true)`; if it returns null (there
>   was no back window), also emit `printStreamCommand(STR_CMD_FLUSH, *ux_stdout)`.
>   `BFC_EXIT` (2): `printStreamCommand(STR_CMD_EXIT, *ux_stdout)` and `return`
>   from the function. `BFC_IGNORE` (3): `printStreamCommand(STR_CMD_IGNORE, ...)`.
>   `BFC_RESUME` (4): `printStreamCommand(STR_CMD_RESUME, ...)`.
> - `BFP_TEXT` (3): read the UString via `*static_cast<UString*>(packet.payload)`
>   (points at the `text` member) and `printPlainTextLine(text, *ux_stdout)`.
> After the loop finishes (EOF), call `flush(false)` to drain and print any
> remaining buffered windows.

> [spec:cg3:def:binary-applicator.cg3.binary-command-type]
> enum BinaryCommandType : uint8_t {
>   BFC_FLUSH = 1;
>   BFC_EXIT = 2;
>   BFC_IGNORE = 3;
>   BFC_RESUME = 4;
> }

> [spec:cg3:def:binary-applicator.cg3.binary-format-flags]
> enum BinaryFormatFlags {
>   BFW_DEP_SPAN = (1 << 0);
>   BFC_RELATED = (1 << 0);
>   BFR_SUBREADING = (1 << 0);
>   BFR_DELETED = (1 << 1);
>   BFV_SETVAR = 1;
>   BFV_SETVAR_ANY = 2;
>   BFV_REMVAR = 3;
> }

> [spec:cg3:def:binary-applicator.cg3.binary-packet]
> struct BinaryPacket {
>   BinaryPacketType type = BFP_INVALID;
>   void* payload = nullptr;
> }

> [spec:cg3:def:binary-applicator.cg3.binary-packet-type]
> enum BinaryPacketType : uint8_t {
>   BFP_INVALID = 0;
>   BFP_WINDOW = 1;
>   BFP_COMMAND = 2;
>   BFP_TEXT = 3;
> }

