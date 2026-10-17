# DominoKit — Implementation Brief and Clipboard Format Specification

**Audience:** an AI coding assistant that will build this program from scratch in Rust.
**Status of this document:** the single source of truth for the task (revision 2). It is derived from two sources plus the project requirements below:

1. a **reference implementation** (`domino_clip.py`, a Python module from another application that already reads and writes Domino's clipboard format), and
2. a **chunk reference** supplied by the project owner (a more complete description of Domino's file/clipboard format, referred to below as "the chunk reference").

Anything not backed by either source is explicitly marked.

Confidence markers used throughout:

| Marker | Meaning |
|---|---|
| **[V]** | Verified: stated by or directly decoded from the reference implementation's real bytes. |
| **[D]** | Documented: stated in the chunk reference. Where a field size could be cross-checked against real bytes it agreed (14 of 15 documented track fields; the one disagreement is in §2.8). |
| **[I]** | Inferred: a reasonable reading of the evidence, not proven. Do not build behaviour that depends on it without a fallback. |
| **[?]** | Unknown: no source tells us. Preserve bytes, don't interpret. |

---

## 1. What you are building

**Name:** `DominoKit` (binary `dominokit`, core library crate `dominokit-core`).
*Domino* is a Windows MIDI sequencer/editor. DominoKit is a small desktop utility that sits beside it and transforms note data that the user copies out of Domino, then hands it back through the clipboard.

**Core workflow (the user performs every copy and paste themselves):**

1. In Domino: select notes, press **Ctrl+C**.
2. In DominoKit: press **Read from Domino clipboard**. The clip is decoded and shown.
3. Configure a **Flip** and/or a **Split**; the preview updates live.
4. Press **Write to Domino clipboard**.
5. In Domino: place the cursor / highlight the target track, press **Ctrl+V**.

### 1.1 Features (version 1)

1. **Split** the clip's contents into parts, by:
   - **Every N ticks** (absolute tick length).
   - **Musical division** (whole, half, quarter, 8th, 16th, 32nd, 64th; plus dotted and triplet variants; plus bars). Requires the clip's PPQ (see §2.4).
   - **Every N note onsets** (count-based; optional extra, cheap to add).
   - **Custom user-defined pattern** (a repeating cycle of lengths, each optionally routed to an output track or dropped; see §4.4). Patterns can be saved, named, reloaded.
2. **Flip vertically** (pitch inversion): every note key `k` becomes `127 − k` (0 ↔ 127, 1 ↔ 126, …). See §4.2.
3. **Inspector** (read-only developer tab): shows the decoded item tree with hex, so unknown items in real Domino clips can be examined. Essential because the format is only partially understood.

### 1.2 Hard constraints on clipboard behaviour (non-negotiable)

- DominoKit touches the clipboard **only** to exchange Domino's own clipboard format, `MidiPortalSequence` (§2.1). No other clipboard format is ever read, enumerated, written, or logged (no text, no images, no file lists).
- The clipboard is accessed **only in direct response to an explicit button press** (Read / Write). No reading at startup, no polling, no clipboard-change listener (`AddClipboardFormatListener` etc.), no background threads touching the clipboard.
- DominoKit **never** simulates keystrokes (no `SendInput`, no synthesized Ctrl+C / Ctrl+V), never sends window messages to Domino, never reads or writes Domino's process memory, never injects anything. The user copies and pastes.
- Writing **replaces** whatever is on the clipboard (the reference does the same). The UI should say so next to the Write button.
- No network access, no telemetry, no files written other than the user's settings file (§6.5).

### 1.3 Non-goals for version 1

- No `.mid`, `.dms` or `.dmp` import yet (planned: §9). But the architecture must make them a drop-in addition.
- No playback, no editing of individual notes, no tempo/time-signature handling beyond the user-supplied values in §4.3.
- No editing of Domino's track/song settings (they are copied byte-for-byte). The documented ones are *read* for display (track name, channel) and for the Inspector.

### 1.4 Interpretation decisions you must follow (the requirements were terse)

- *"Split by absolute notes like every N ticks"* is implemented as **absolute tick lengths**, plus the optional "every N note onsets" mode as the count-based reading.
- *"Split"* produces **parts**. What happens to the parts is a separate **output mode** (§4.3): browse them one at a time as separate clips, or lay them out as tracks of one multi-track clip.
- *Flip* and *Split* are independent and **commute** (flip changes pitch only, split changes time only), so the UI offers them as two toggleable stages in a fixed pipeline: `source → flip → split → output`.

---

## 2. The clipboard format

### 2.1 Envelope **[V]**

Clipboard format: a **registered (custom) clipboard format** whose registered name is the string

```
MidiPortalSequence
```

(Win32: `RegisterClipboardFormatW(L"MidiPortalSequence")`; the returned id is process-/session-specific, so always register by name, never hard-code the number.)

**The same envelope is used by `.dms` project files [D]**: the chunk reference gives the `.dms` header as magic `PortalSequenceData`, then a u32 uncompressed size, then zlib data, which is exactly the clipboard layout. A `.dmp` (part) file uses the same layout with magic `PortalPartData` **[D]**. So the clipboard payload is, in effect, a `.dms` file held in memory.

Payload bytes:

| Offset | Size | Field |
|---|---|---|
| 0 | 18 | ASCII magic `PortalSequenceData` (`50 6F 72 74 61 6C 53 65 71 75 65 6E 63 65 44 61 74 61`), no terminator. (`.dmp` files: `PortalPartData`, 14 bytes; not used on the clipboard in the reference.) |
| 18 | 4 | `unpacked_size`, u32 little-endian = length of the decompressed data |
| 22 | … | zlib stream (RFC 1950, with zlib header) of the **chunk data** (§2.2) |

Rules for the Rust implementation:

- All integers anywhere in the format are **little-endian**.
- **Decoding:** check magic, read `unpacked_size`, decompress with `flate2::read::ZlibDecoder`, verify the output length equals `unpacked_size`. **Ignore any bytes after the end of the zlib stream**: Windows clipboard memory blocks can be larger than the data placed in them.
- **Encoding:** `flate2` default compression is fine. Compressed bytes need not match any other encoder's output byte-for-byte; compare **unpacked** bytes in tests, never compressed bytes.
- Make the magic a parameter of the envelope codec (`PortalSequenceData` | `PortalPartData`), so the future file importers reuse it.
- Errors to distinguish for the UI: format absent from clipboard / present but wrong magic ("not Domino data") / zlib failure ("damaged") / size mismatch.

### 2.2 Chunk grammar **[V][D]**

The decompressed data is a flat sequence of **chunks**, repeated to the end of the stream:

```
chunk := id:u16  size:u32  data:[u8; size]      // 6-byte header; size excludes the header; id >= 1000
```

(The reference implementation calls these "items"; same thing.) A **container** is a chunk whose data is itself a chunk sequence.

**IDs are scoped by their parent.** The same number means different things at root level, inside a track, and inside an event (e.g. `1001` is "copyright" at the root, "channel" in a track, "position" in an event). Always interpret by **path**, never by ID alone.

**Value types** used in the tables below:

| Type | Encoding |
|---|---|
| `u8` / `u16` / `u32` | unsigned, little-endian |
| `s8` / `s16` / `s32` | signed two's complement, little-endian |
| `string` | raw bytes filling the chunk: **no terminator, no length prefix** **[V]** (the template's 17-byte `General MIDI Drum` is exactly the chunk size). Text encoding is **unknown [?]** (likely a legacy code page). Store as `Vec<u8>`, display lossily, never re-encode. |
| `raw` | opaque bytes |
| `float` | width/count unknown **[?]** (tempo only); opaque |

Decode a field by its **actual chunk size**, not only by its documented type (see the `1015` discrepancy in §2.8).

Parsing must be tolerant and lossless:

- Parse the root into chunks. Recurse into tracks (`1003`) and into event containers you need to edit (§2.3). Every other chunk body stays an **opaque byte blob, re-emitted verbatim**, in its original position (order is meaningful: don't sort or normalise).
- If a chunk's declared size runs past the end of its parent, stop and keep the remaining bytes as `trailing: Vec<u8>` and raise a warning.
- Re-serialisation recomputes only the sizes of chunks you modified or rebuilt, plus the envelope's `unpacked_size`.
- Implement the tables in §2.3 as a **data-driven schema** (`schema.rs`: path → name, type, container-or-not). The schema drives the Inspector, event handling and validation. Unknown paths fall back to opaque.

### 2.3 Structure of a clip

**Root** (top level of the unpacked data):

| ID | Type | Name | Notes |
|---|---|---|---|
| 1000 | string | name | [D]; empty in the template [V] |
| 1001 | string | copyright | [D]; empty in the template [V] |
| **1002** | u16 | **PPQ** | [D][V]: ticks per quarter note of the song the clip came from (§2.4) |
| **1003** | container | **track** (one per track) | [D][V] |

The reference's template also contains **root chunks the chunk reference does not document**; they are real and must be preserved in place **[V]**. Observed template values, in order:
- *before the tracks:* 1007 u32 `48`, 1009 u32 `0`, 1012 8 × `00`, 1013 u32 `0`, 1014 u32 `0`, 1019 empty, 1020 u32 `16`, 1021 u8 `1`, 1022 u8 `1`, 1023 u32 `17`, 1024 u8 `1`;
- *after the tracks:* 1006 empty; 1008 container (`1000` empty, `1001` u32 `1`, `1002` u32 `1`); 1017 container (scoped IDs 100–106: u8 0, u8 0, u8 0, u32 100, u8 0, u32 100, 12 × `05`).

All of these are [?] in meaning. A real `.dms` project root will likely contain more (tempo map, etc.); preserve whatever is there.

**Track container (1003).** Documented fields **[D]** with the values the reference's template uses **[V]**. The template's physical order (which you must preserve) is: 1000, 1001, 1002, 1003, 1004, 1008, 1009, 1011, 1012, 1016, 1017, 1013, 1014, 1015, 1018, 1019, 1020, 1021, 1022, *events…*, 2009, 1005, 1006, 1007, 1010.

| ID | Type | Name [D] | Template value [V] |
|---|---|---|---|
| 1000 | u16 | out port | 0 |
| 1001 | u8 | **channel** | 0 |
| 1002 | string | name | empty |
| 1003 | u8 | *(undocumented)* | 0 |
| 1004 | u8 | is rhythm | 0 |
| 1005 | u32 | *(undocumented)* | 50 |
| 1006 | u8 | volume | 100 |
| 1007 | s32 | gate | 480 |
| 1008 | u8 | *(undocumented)* | 60 |
| 1009 | string | rhythm name | `General MIDI Drum` |
| 1010 | container | *(undocumented)* | `1000` u8 0, `1001` u8 0 |
| 1011 | u16 | *(undocumented)* | 0 |
| 1012 | u32 | tick adjust | 0 |
| 1013 | u8 | *(undocumented)* | 1 |
| 1014 | u16 | *(undocumented)* | 1 |
| 1015 | s32 per [D] | enabled | **1 byte** `01` observed (§2.8) |
| 1016 | s32 | gate adjust | 100 |
| 1017 | s32 | transpose | 0 |
| 1018 | u8 | color (255 = none) | 255 |
| 1019 | u32 | tick adjust measure | 1 |
| 1020 | raw | *(undocumented)* | 16 × `00` |
| 1021 | u8 | range low | 0 |
| 1022 | u8 | range high | 127 |
| 2009 | container | *(undocumented in [D])* **copied-stretch length** | `1001` = u32 length, see §2.6 |

**Event containers** inside a track (IDs 2001 and up) **[D]**. *Every documented event starts with `1001: u32 pos` (position in ticks).* This is what lets split (§4.3) place and shift every event, not just notes.

| ID | Event | Fields (inside the event; IDs scoped to it) |
|---|---|---|
| **2001** | **note** | 1001 u32 position · **2001 u8 key** · 2002 u8 volume (= velocity) · 2003 u32 duration (= gate) |
| 2002 | program change | 1001 u32 pos · 2001 s8 *(unknown)* · 2002 s8 *(unknown)* · 2003 u8 patch · 2004 raw *(unknown)* · 2005 u8 *(unknown)* · 2006 s16 *(unknown)* |
| 2003 | control | 1001 u32 pos · 2001 u16 cc · 2002 raw data1 · 2003 raw data2 |
| 2004 | sysex | 1001 u32 pos · 2001 string text · 2002 raw sysex |
| 2005 / 2011 / 2012 / 2017 | text / lyric / cue point / marker | 1001 u32 pos · 2001 string text |
| 2007 | expression | 1001 u32 pos · 2001 string var · 2002 string value |
| 2008 | tempo | 1001 u32 pos · 2001 float value(s) — width/count uncertain [?] |
| 2014 | measure link | 1001 u32 pos · 2001 u32 measure_dest · 2002 s32 key_transpose |
| 2015 | time signature | 1001 u32 pos · 2001 u8 num · 2002 u8 denom (spelled "nenom" in the source; the denominator's encoding — literal 4 vs exponent 2 — is unknown [?]) |
| 2016 | key signature | 1001 u32 pos · 2001 u8 key |
| 2018 | key scale | 1001 u32 pos · 2001 u8 key · 2002 u8 chord · 2003 u8 × 12 custom · 2004 string name |
| 2019 | chord | 1001 u32 pos · 2001 u8 key · 2002 u32 chord · 2003 raw custom · 2004 string name |
| 2006, 2010, 2013, ≥ 2020 | *(undocumented)* | unknown; see §4.7 for how they are treated |

Notes about tracks:

- **Channel and port are stored (`1001`, `1000`) [D], but paste ignores them [V]:** per the reference, Domino puts the first copied track into the *highlighted* track and the following copied tracks into the tracks below it, and tracks beyond the last existing track are dropped. Track settings in the clipboard are therefore *probably* not applied on paste **[I]**; do not rely on editing them. Use name/channel/port only for **display** (label tracks in the preview as e.g. "Track 2 · ch 10 · Drums"). DominoKit cannot know how many tracks exist below the user's selection, so warn when writing more than one track.
- Real Domino copies may contain any of the event types above. The reference reader skipped everything except notes **[V]**; DominoKit preserves them (flip) or shifts them (split).
- Not every track necessarily has `2009`; handle its absence (§2.6).

### 2.4 Time base **[V]**

- Ticks are counted **from the start of the copied stretch**: tick 0 is where the copy began, not the start of the song. Event positions (`1001`) use the same origin.
- Domino pastes the stretch so that its start lands at Domino's **play cursor**.
- **Domino does not convert ticks to its own PPQ on paste**; the user is responsible for the destination project having the same PPQ. Therefore DominoKit must **never rescale ticks**, must show the clip's PPQ prominently, and should remind the user to match it.
- PPQ comes from root chunk `1002` (u16). If absent (should not happen), PPQ is unknown: disable musical-division modes until the user types a PPQ.
- **Time signature:** there is no global field. Time-signature **events** (`2015`, §2.3) exist in tracks **[D]** and may or may not be present in a given copy. Bar-based divisions use, in order: (1) the user's explicit entry; (2) otherwise the first `2015` event in the clip, shown as a *prefilled, editable* value — but because the denominator's encoding is unverified [?], only prefill when `denom` ∈ {1, 2, 4, 8, 16, 32, 64} (literal reading) and label it "detected (unverified)"; (3) otherwise 4/4.

### 2.5 Note item (tag 2001, length 34) **[V]**

Documented names **[D]**: position (`1001`), key (`2001`), volume (`2002`, this spec says *velocity*), duration (`2003`, this spec says *gate*). Canonical layout, exactly 40 bytes including the note's own 6-byte header:

| Bytes | Field |
|---|---|
| `D1 07 22 00 00 00` | note item: tag 2001, length 34 |
| `E9 03 04 00 00 00` + u32 | tag 1001, **start tick** (u32), relative to the copied stretch |
| `D1 07 01 00 00 00` + u8 | tag 2001, **key** (u8, MIDI note number 0–127) |
| `D2 07 01 00 00 00` + u8 | tag 2002, **velocity** (u8, 1–127) |
| `D3 07 04 00 00 00` + u32 | tag 2003, **gate** (u32 ticks of duration; ≥ 1) |

Reader leniency in the reference **[V]**: a note in a different layout (different field order, extra fields) is still accepted if it has `1001` (4 bytes), `2001` (1 byte) and `2003` (4 bytes); a missing `2002` means velocity 100. Keys > 127 are discarded by that reader; gate < 1 is raised to 1; velocity is clamped to 1–127. **DominoKit must not apply those normalisations on an identity or flip path**; it preserves what it read. Apply them only to notes it *creates*.

Writer behaviour **[V]**: notes within a track are sorted by `(start tick, key)` ascending.

### 2.6 Copied-stretch length (chunk 2009) **[V]**

Each track in the reference's output carries `2009 { 1001: u32 length }` after its events: the length in ticks of the copied stretch. The chunk reference does **not** list `2009` [?], so it may be specific to clipboard/part data and may be absent from `.dms` files. The reference gives **every track the same value** ("every track shares that stretch"). Notes may have a gate that runs past it. The length is what a paste spans/advances by [I], so any transform that changes the time layout must set it deliberately (§4.3). If a clip has no `2009`, treat the stretch length as `max(pos + duration)` over its notes and events when you need a number, and when *writing* a clip for the clipboard always include `2009` (create it from the fallback template if absent).

The reference writer defines the stretch two ways **[V]** (useful for the future imports, and for understanding clips you read):
- "bar" start: stretch starts at the bar line at or before the first note, ends at the next bar line at/after the last note's end (min one bar).
- "note" start: stretch starts at the first note's start tick and ends at the last note's end tick (min 1), so the first note pastes exactly at the cursor.

### 2.7 Data model to implement (recommended)

Goal: **`encode(decode(x))` reproduces the unpacked bytes of `x` exactly** when no transform is applied.

```rust
pub struct Item { pub tag: u16, pub body: Vec<u8> }               // one chunk (id + body), opaque, verbatim

pub enum TopNode {
    Raw(Item),                    // every song-level item, kept in original position
    Track(Track),                 // tag 1003
}

pub struct Track {
    pub children: Vec<TrackChild>,
}
pub enum TrackChild {
    Raw(Item),                    // settings (1000-1022), anything undocumented
    Note(NoteItem),               // 2001
    Event(EventItem),             // any other event container (2002..), see below
    Length(LengthItem),           // 2009 (holds the u32 from its inner 1001)
}

/// Any non-note event. `pos` is the value of its inner `1001` (u32) when present.
/// Everything else in the body is kept verbatim; only `pos` is ever patched.
pub struct EventItem { pub id: u16, pub pos: Option<u32>, pub item: Item }

/// Lossless note: parsed view + original child items (so unknown extra fields survive).
pub struct NoteItem { pub tick: u32, pub gate: u32, pub key: u8, pub vel: Option<u8>, pub extra: Vec<Item> /* + layout info */ }

pub struct Clip { pub nodes: Vec<TopNode>, pub trailing: Vec<u8> }
impl Clip {
    pub fn ppq(&self) -> Option<u16>;
    pub fn tracks(&self) -> impl Iterator<Item = &Track>;
    pub fn stretch_len(&self) -> Option<u32>;       // from the first track's 2009
}
```

Implementation notes:
- Keep `u32` for tick/gate on the wire, but do arithmetic in `i64` and use checked conversion back to `u32`; refuse to write on overflow.
- Provide a flat read-only view for the UI/preview: `Vec<NoteView { track: usize, tick, gate, key, vel }>`, and a list of `(track, event id, pos)` for drawing event markers.
- A **fallback track template** (for tracks DominoKit must synthesise, e.g. the future `.mid` import) is given in Appendix A. When creating extra tracks from an existing clip (split → tracks), **clone the first source track's settings and tail chunks** (everything that is neither a note nor an event) and replace only its notes/events and its 2009 value; do not use the Appendix A template unless the clip has no tracks.

### 2.8 Where the two sources disagree (and what to do)

| Topic | Reference implementation | Chunk reference | Handling |
|---|---|---|---|
| Is the channel in the data? | Comment says it isn't; paste goes into the highlighted track. | Track `1001` = channel. | Both can be true: the field exists, paste ignores it. Display it; don't depend on it (§2.3). |
| Size of track `1015` (enabled) | Real bytes: **1 byte** (`01`). | `s32`. | Decode by actual size; preserve bytes; never rewrite it. |
| Chunk `2009` | Present in every track, holds the stretch length. | Not listed. | Treat as clipboard/part-specific (§2.6). |
| Root chunks 1007–1024 and 1006/1008/1017; track 1003/1005/1008/1010/1011/1013/1014/1020 | Present in the template. | Not listed. | Opaque, preserved verbatim. |
| Envelope for `.dms` | — | Same magic/layout as the clipboard. | One envelope codec for both (§2.1). |

---

## 3. Architecture

Cargo workspace:

```
dominokit/
  Cargo.toml                       # [workspace]
  crates/
    dominokit-core/                # pure logic: NO GUI, NO OS APIs, fully unit-testable
      src/format/{envelope.rs, chunk.rs, schema.rs, clip.rs, template.rs}
      src/ops/{flip.rs, split.rs, grid.rs, pattern.rs}
      src/lib.rs
    dominokit/                     # the application
      src/main.rs
      src/app/{mod.rs, preview.rs, panels.rs, inspector.rs, state.rs}
      src/clipboard/{mod.rs, windows.rs, unsupported.rs}
      src/settings.rs
```

Suggested dependencies: `eframe`/`egui` (GUI), `flate2` (zlib), `thiserror` (core errors), `anyhow` (app), `serde` + `serde_json` (settings, pattern presets), `directories` (config path), `tracing`; Windows clipboard via `clipboard-win` **or** the `windows` crate (verify the exact API of whichever version you pin); `hex-literal` (constants). Later: `midly` (`.mid`). Dev: `proptest`.

**Clipboard abstraction** (so the app is testable and compiles on non-Windows):

```rust
pub trait DominoClipboard {
    /// Ok(None) = our format isn't on the clipboard. Err(Busy) after bounded retries.
    fn read(&mut self) -> Result<Option<Vec<u8>>, ClipError>;
    fn write(&mut self, raw: &[u8]) -> Result<(), ClipError>;
}
```

Windows backend, Win32 level (whatever crate you use must amount to this):
- `RegisterClipboardFormatW("MidiPortalSequence")` once.
- **Read:** `IsClipboardFormatAvailable(fmt)` first (distinguishes "absent" from "busy"); `OpenClipboard` with a few retries (e.g. 10 × 20 ms; another app may hold it); `GetClipboardData(fmt)` → `GlobalLock` → copy `GlobalSize` bytes (may include padding) → `GlobalUnlock` → `CloseClipboard` (always, even on error; use an RAII guard).
- **Write:** same retry on `OpenClipboard`; `EmptyClipboard`; `GlobalAlloc(GMEM_MOVEABLE, len)` → lock, copy, unlock; `SetClipboardData(fmt, hmem)` (ownership passes to the system on success; free it yourself on failure); `CloseClipboard`. Write **only this one format**.
- Never call any other clipboard API (no `EnumClipboardFormats`, no listener).
- Non-Windows: `unsupported.rs` returns `ClipError::Unsupported`; the GUI still runs (so the app can be developed elsewhere), with Read/Write disabled and a message.

---

## 4. Operations (the behaviour to implement)

State machine: **Source clip** (decoded from clipboard, never mutated) → **Flip stage** → **Split stage** → **Output** (what Write sends). Changing any option recomputes from the source. A "Use output as new source" button lets the user chain further operations.

### 4.1 Common rules

- Operate on the structural model of §2.7; unrelated items are preserved byte-for-byte.
- Identity (no stage enabled) must write back a clip whose **unpacked bytes equal the source's**. Do not re-sort notes unless a stage changes ticks across tracks/parts.
- Don't write an output with zero notes; show "nothing to write".
- Always **self-check before writing**: encode → decode → compare note count against the output model; refuse and report on mismatch.

### 4.2 Flip (vertical)

```
for each selected track t (default: all tracks):
    for each note n in t:
        if n.key <= 127: n.key = 127 - n.key        // 0<->127, 1<->126, ... 63<->64
        else:            leave untouched, count as warning
```

- Only `key` bytes change. Ticks, gates, velocities, order, all non-note events and settings, PPQ, and the stretch length are untouched.
- It is an **involution**: `flip(flip(x)) == x` byte-for-byte; make this a test.
- Options (UI): per-track checkboxes. *Optional extras (implement after the above is solid):* axis = `Full range (127−k)` (default) | `Clip range (min+max−k)` | `Custom lo..hi (lo+hi−k)`; for custom axes, notes that would land outside 0–127 follow a user policy (clamp / drop / skip-the-flip), default clamp, and are counted in a warning.
- Only **notes (`2001`)** are flipped. All other events pass through unchanged. In particular, `key` fields in key signature (`2016`), key scale (`2018`) and chord (`2019`) are tonal *labels*, not MIDI note numbers, and mirroring them is a music-theory decision this tool does not make; leave them alone and mention this in the Flip panel's help text.
- Track settings are untouched on the clipboard path (paste ignores them, §2.3). *(Future, for whole-project `.dms` flipping: also mirror a track's range as `low' = 127 − high`, `high' = 127 − low`.)*

### 4.3 Split

Splitting is two decisions: **where to cut** (§4.4–4.6) and **what to do with the parts** (§4.7). Terminology: the clip's time span is `[0, span)` where `span = max(stretch_len, max(pos + 1) over notes and events)`. A **part** is a half-open tick range `[a, b)` of that span, optionally routed to an output track number (`target`) or dropped.

Global settings used by the split stage:

| Setting | Meaning | Default |
|---|---|---|
| PPQ | From the clip; editable only if missing. | clip's `1002` |
| Time signature (n/d) | Only used for "bar" lengths; see §2.4 for how it is prefilled. | 4/4 |
| Grid offset | Ticks added to the grid origin, so cuts can be shifted. Cuts before 0 are ignored. | 0 |
| Boundary policy | What happens to a note that starts in one part and sounds past the part's end: **Keep** (stays whole in the part its start tick is in; gate untouched), **Truncate** (gate cut to `part_end − tick`), **Slice** (split into pieces, one per part it crosses; continuation pieces start exactly at the boundary, same key/velocity, gate = their share). | Keep |
| Position policy | **Rebase**: part notes' ticks become `tick − part_start` and the part's stretch length becomes `part_end − part_start`. **Keep position**: ticks unchanged, stretch length = original span (the part pastes where it came from). | Rebase |
| Skip empty parts | Parts with no **notes** are omitted (their events are dropped with them). | on |
| Events | **Follow position**: every event container with a position goes to the part its position falls in, shifted like notes. **Drop**: only notes are kept. | Follow position |
| Max parts | Safety cap to protect the UI. | 4096 |

A note or event belongs to the part that contains its **start tick / position** (before Slice pieces are added). Ties and chords are never separated by Keep/Truncate because chord notes share a start tick.

### 4.4 Cut definitions

All produce a sorted, de-duplicated list of **cut points** `c0 < c1 < … < ck` with `c0 = 0` (after offset) covering the span; parts are `[c_i, c_{i+1})`; the last part ends at `max(span, c_k)`.

**(a) Every N ticks:** `c_i = offset + i·N`, `N ≥ 1`.

**(b) Musical division:** choose a base value and modifier, convert to an exact rational number of ticks, then compute each cut from the origin (do not accumulate rounded steps, to avoid drift):

```
whole-note ticks W = 4 * ppq
base fraction  f ∈ {1/1, 1/2, 1/4, 1/8, 1/16, 1/32, 1/64}
modifier       plain ×1 | dotted ×3/2 | triplet ×2/3
bar            = W * n / d  (time signature n/d)
step (rational) = W * f * modifier           // e.g. 1/16 with ppq 480 → 120 ticks
c_i = offset + round_half_up(i * step)       // integer arithmetic on the rational
```

Examples at PPQ 480: quarter = 480, eighth = 240, sixteenth = 120, triplet-eighth = 160, dotted-quarter = 720.
If `step` rounds to 0 ticks (e.g. 1/64 at PPQ 24), reject with a message.

**(c) Every N note onsets:** collect distinct start ticks of all notes in all selected tracks (notes sharing a tick count as one onset), sorted: `t0 < t1 < …`. Cuts at `0`, `t_N`, `t_{2N}`, … (N ≥ 1). Leading empty time before `t0` is part 0 if "Skip empty parts" is off.

**(d) Custom pattern** — §4.5.

### 4.5 Custom user-defined patterns

A **pattern** is a named, ordered list of **steps** that repeats from the grid origin until the span is covered:

```
Step { length: Length, target: Target }
Length = Ticks(u32) | Note{ num, den, modifier } | Beats(u32) | Bars(u32)
Target = Track(u16 /*1-based output track group*/) | Drop
```

- A cycle's total length must be > 0 and every step ≥ 1 tick after conversion; otherwise reject.
- Cut points are the running sum of step lengths (computed with the exact-rational rule of §4.4b per cycle, then offset by whole cycles).
- `Drop` parts are discarded (notes that start in them vanish; under Slice, pieces landing in dropped parts vanish).
- `Track(n)` matters for output mode "Parts as tracks" (§4.7); in "Separate clips" mode it is ignored except that `Drop` still hides the part.

**Text form** (for presets and for a quick one-line edit box; the GUI should *also* provide a table editor with one row per step: value, unit, target):

```
pattern := step { (',' | whitespace) step }
step    := length [ ':' target ]
length  := INT 't'                        // ticks            480t
         | INT '/' INT [ '.' | 'T' ]      // note value       1/16   1/8.  (dotted)   1/8T (triplet)
         | INT 'b'                        // beats (quarters) 2b
         | INT 'bar'                      // bars             1bar
target  := INT | '-'                      // output track 1.., or '-' to drop
```

Examples: `1/8:1 1/8:2 1/4:-` (tresillo-like routing: first eighth → track 1, second eighth → track 2, then a quarter dropped); `3/16 3/16 2/16` (3+3+2 sixteenth grouping, no routing); `1/16:1 1/16:- 1/16:- 1/16:-` (keep only the first sixteenth of every beat).
Presets are saved in the settings file (§6.5) as `{ name, steps }`.

### 4.6 Worked example (use as a test)

Clip: PPQ 480, one track, notes `(tick, gate, key, vel)` = `(0,240,36,100)`, `(240,240,38,90)`, `(480,480,127,64)`, stretch length 960.

| Split | Cut points | Parts (Keep / Rebase) |
|---|---|---|
| Every 1/4 (480 ticks) | 0, 480, 960 | P0 `[0,480)`: notes at 0, 240 (length 480). P1 `[480,960)`: the note at 480 → rebased tick **0** (length 480). |
| Every 1/8 (240 ticks) | 0, 240, 480, 720, 960 | P0: note@0. P1: note@240. P2 `[480,720)`: note@480 (gate 480 crosses 720). P3: empty (skipped). |
| …same, **Truncate** | | P2's note gate becomes 240. |
| …same, **Slice** | | P2 note: tick 480 gate 240; P3 gets a new piece: tick 720 gate 240 (rebased: 0). |
| …same 1/8 split with a control event at tick 300 (Events = Follow position, Rebase) | | The event goes to P1 `[240,480)`; its `1001` becomes **60**. |
| Pattern `1/8:1 1/8:2 1/4:-`, tracks mode, **Keep position** | 0, 240, 480 (cycle = 960) | Output track 1: note@0. Output track 2: note@240. `[480,960)` dropped. Stretch length 960 on both. |

### 4.7 Output modes

1. **Separate clips** (default): the result is a list of parts. The user selects one and presses **Write part k**; also **Write & next** (writes the part after the selected one and selects it; makes paste-then-advance workflows quick). Each clip contains the source's tracks (each reduced to that part's notes).
2. **Parts as tracks:** one multi-track clip. Parts are distributed to *output track groups* by `target` (patterns) or by part index (non-pattern modes: part *i* → group *i+1*). Option **Round-robin K**: part *i* → group *(i mod K)+1*. If the source has *S* tracks, each group has *S* tracks, ordered group-major: output track index = `(group−1)·S + source_track`. Every output track gets the same stretch length (Rebase: the longest part length; Keep position: the original span) **[V: tracks share the stretch]**. Parts routed to the same group are merged into that group's track (stable sort by tick). Show a warning with the number of tracks the paste will need.

For every output track: start from a **clone of the corresponding source track** (settings and tail chunks verbatim), delete its notes and events, insert the part's notes and events, set its `2009` length.

**Events in split outputs.** Every documented event has a `1001` position **[D]**, so with *Events = Follow position*:
- the event is assigned by position exactly like a note; under **Rebase** its `1001` is rewritten to `pos − part_start`, under **Keep position** it is unchanged; nothing else in the event is touched;
- Slice/Truncate affect notes only;
- events with undocumented IDs (2006, 2010, 2013, ≥ 2020) are treated the same way **only if** their body parses cleanly as chunks and contains a `1001` of size 4 **[I]**; otherwise they are dropped and counted;
- **Warn the user that state-type events are not carried across parts:** a part that starts mid-song does not inherit the preceding program change, control values, tempo, time signature or key signature, because DominoKit doesn't synthesise them. (A "carry state forward" option is future work.)

Notes created by Truncate/Slice must satisfy gate ≥ 1 (drop pieces that would be 0).

---

## 5. Test vectors and acceptance tests

### 5.1 Golden clipboard payload

A real-format clip produced by the reference writer (PPQ 480, "note" start, one track, 3 notes, length 960). Notes `(tick, gate, key, vel)`: `(0,240,36,100)`, `(240,240,38,90)`, `(480,480,127,64)`.

**Complete clipboard payload** (envelope + zlib; 303 bytes). Your decoder must accept it:

```
506f7274616c53657175656e63654461746156020000789c6d503d4f0251107ceb628e1863b0b522c6581b6b0b63480c
857fc0cee44e85280589357fcdbf80567694280a87f2292aeece720b896eb23bfbe6e6edcddb36078967d4175e93daa2
1ee7040f24fbe834069c0fcb183a3ff26e86195f381724bf99a4d28fc11cfcb64a72203a5c116ce3976a80cc81d68e1d
5e0d52c0519ff5f269524bea1737c5b372a95c2cd5ef6e3f160306ee63822e969c3a3734132388298cedf80998cf2023
982f84bf610f09f6904633da85df6c7433527aef01103f46caa7e097bad475fba63bff5fd7a24cd730ddb1e9947f8a36
5c772f6b7a437728d985b1b8e723de792be86a6965b35db65deee04bf05184ad67dd944fb03b5c490c2e0dae42b6d66b
232a4e54c3a6d4f595f805fa9e5143
```

**Its unpacked chunk data** (598 bytes; compare against this):

```
e80300000000e90300000000ea0302000000e001ef030400000030000000f1030400000000000000f403080000000000
000000000000f5030400000000000000f6030400000000000000fb0300000000fc030400000010000000fd0301000000
01fe030100000001ff03040000001100000000040100000001eb0369010000e803020000000000e9030100000000ea03
00000000eb030100000000ec030100000000f003010000003cf1031100000047656e6572616c204d494449204472756d
f303020000000000f4030400000000000000f8030400000064000000f9030400000000000000f5030100000001f60302
0000000100f7030100000001fa0301000000fffb030400000001000000fc031000000000000000000000000000000000
000000fd030100000000fe03010000007fd10722000000e9030400000000000000d1070100000024d2070100000064d3
0704000000f0000000d10722000000e90304000000f0000000d1070100000026d207010000005ad30704000000f00000
00d10722000000e90304000000e0010000d107010000007fd2070100000040d30704000000e0010000d9070a000000e9
0304000000c0030000ed030400000032000000ee030100000064ef0304000000e0010000f2030e000000e80301000000
00e9030100000000ee0300000000f0031a000000e80300000000e9030400000001000000ea030400000001000000f903
420000006400010000000065000100000000660001000000006700040000006400000068000100000000690004000000
640000006a000c000000050505050505050505050505
```

Expected decode: ppq = 480; one track; stretch length = 960 (`C0 03 00 00`); notes as above in this order; the three note items are the 40-byte canonical form, e.g. the first is
`D1 07 22 00 00 00  E9 03 04 00 00 00 00 00 00 00  D1 07 01 00 00 00 24  D2 07 01 00 00 00 64  D3 07 04 00 00 00 F0 00 00 00`.

Expected after **Flip**: keys `91, 89, 0` (36→91, 38→89, 127→0); every other byte of the unpacked data unchanged.

### 5.2 Required tests

1. Decode golden → fields above; **identity round trip** → unpacked bytes identical to §5.1.
2. Envelope: corrupt magic, truncated zlib, wrong `unpacked_size`, trailing padding after the zlib stream (must be accepted).
3. Flip involution (property test over random keys); flip leaves all non-key bytes unchanged.
4. Split: all rows of §4.6; plus property tests: under **Keep**, the multiset of `(tick, key, vel, gate)` across all parts (with Keep position) equals the source's; under **Slice**, total sounding duration per key is preserved.
5. Grid math: PPQ 480 / 96 / 120 / 960 for every base value and modifier; no drift over 10 000 cuts for triplets at PPQ 480 (`step = 160`) and at PPQ 100 (non-integer steps).
6. Pattern parser: valid and invalid strings (zero lengths, missing target, `1/0`, empty).
7. Output mode "Parts as tracks" with S = 2 source tracks yields the group-major order of §4.7.
8. A clip containing an unknown extra chunk inside a track and inside a note round-trips unchanged under identity and flip.
9. Synthetic clips containing each documented event type (build chunk bytes by hand from §2.3): identity round trip is byte-exact; flip changes only note keys; split with Follow position rewrites only the event's `1001` and keeps every other byte; a `1015` chunk of 1 byte and one of 4 bytes both survive untouched.
10. `.dms`-style input: a payload with a different set of root chunks than the golden clip decodes and re-encodes identically.

### 5.3 Manual acceptance (with Domino open; you may not be able to run it — write it into the README)

1. Copy notes in Domino → Read → Write → paste: result must be indistinguishable from pasting the original copy.
2. Flip → paste: pitches mirrored.
3. Split every quarter note, Separate clips → Write part 1 → paste at cursor; repeat for part 2.
4. Parts as tracks → highlight a track with enough empty tracks below → paste.

---

## 6. GUI specification (egui / eframe)

### 6.1 Layout

```
┌───────────────────────────────────────────────────────────────────────────┐
│ DominoKit    [Read from Domino clipboard] [Write to Domino clipboard ▾]   │
│ Clip: 2 tracks · 128 notes · PPQ 480 · length 1920 ticks   (!) match PPQ  │
├──────────────────┬────────────────────────────────────────────────────────┤
│ ☐ Flip           │  Piano-roll preview (pitch 0–127 vertical, time x)     │
│   axis, tracks…  │   • notes coloured by track                            │
│                  │   • cut lines + alternating part shading               │
│ ☐ Split          │   • flip ghost: original notes dimmed                  │
│   mode ▾         │   • hover tooltip: tick / key / vel / gate             │
│   params         │                                                        │
│   boundary ▾     ├────────────────────────────────────────────────────────┤
│   position ▾     │  Parts / Output list (index, range, note count, target)│
│   output ▾       │   [Write part] [Write & next]                          │
│   pattern editor │  Tabs: Preview | Inspector                             │
├──────────────────┴────────────────────────────────────────────────────────┤
│ Status / warnings (dropped events, track-count warning, errors)           │
└───────────────────────────────────────────────────────────────────────────┘
```

### 6.2 Behaviour

- Read/Write are the only controls that touch the clipboard. Write is disabled until there is an output; the Write tooltip says it replaces the clipboard contents.
- Reading shows a friendly message when the format is absent ("Nothing from Domino on the clipboard. In Domino, select notes and press Ctrl+C, then try again.") and a distinct one when busy.
- All parameter edits recompute the preview immediately (debounce only if > ~50k notes). Invalid input (bad pattern text, zero tick step) shows an inline error and keeps the last valid result.
- Preview must stay responsive for ~100k notes (draw only visible notes; decimate when zoomed out).
- Musical-division and bar options are disabled with an explanation when PPQ is unknown; a PPQ field appears.
- Keyboard: `Ctrl+R` read, `Ctrl+W` write (only while DominoKit has focus; these do **not** touch Domino).

### 6.3 Inspector tab

Read-only tree of the decoded chunks for the source clip: path, ID (decimal and hex), the **schema name and type from §2.3 when known** (otherwise "unknown"), size, and a decoded value or hex/ASCII preview; best-effort nested parse of unknown bodies that decode cleanly as chunks (label this "guess"). Do **not** add a "copy to clipboard" button for the dump (clipboard rule §1.2); offer **Save dump to file…** (user-chosen path) instead.

### 6.4 Errors and warnings to surface

Non-Domino data on clipboard; damaged data; unparseable track structure (writing disabled); keys > 127 skipped by flip; undocumented events without a position dropped by split; part-count cap hit; tick overflow; PPQ missing; multi-track write warning ("Domino drops tracks beyond the last existing one").

### 6.5 Persistence

JSON settings in the OS config dir: last-used options, window size, saved pattern presets. Nothing else is stored; clip contents are never saved unless the user uses an explicit "Save dump".

---

## 7. Milestones

1. `dominokit-core`: envelope + item parser + model + golden tests (identity round trip).
2. Flip + tests. Split engine (all cut modes, boundary/position policies, tracks mode) + tests.
3. Clipboard backend (Windows) + `unsupported` stub; trait-based fake for tests.
4. GUI: read/write, status line, preview, flip.
5. GUI: split controls, pattern editor, parts list, output modes, presets.
6. Inspector; polish; README with the manual acceptance steps of §5.3.

**Definition of done:** all §5.2 tests pass; the app builds on Windows (primary) and compiles on Linux/macOS with the clipboard disabled; the clipboard rules of §1.2 hold (grep-able: only one clipboard format is ever named in the code).

---

## 8. Open questions (design for them; do not guess)

- **[?]** Meaning of the undocumented root and track chunks (§2.3, §2.8). Preserve verbatim.
- **[?]** Text encoding of `string` chunks (names, text events, rhythm name). Treat as bytes.
- **[?]** Tempo (`2008`) `float` width/count, and the time-signature (`2015`) denominator encoding. Until verified: preserve; don't synthesise tempo or time-signature events; only prefill bars from `2015` under the guarded rule in §2.4.
- **[?]** Unknown sub-fields in program change (`2002`: 2001, 2002, 2004, 2005, 2006).
- **[?]** Whether Domino's *paste* applies event types other than notes (controls, tempo, …) and whether a *copy* includes them. DominoKit preserves/shifts them regardless; test with real Domino.
- **[?]** Whether `2009` appears in `.dms`/`.dmp` files, and the exact content of `.dmp` (`PortalPartData`) beyond its header.
- **[?]** Whether Domino tolerates missing optional chunks, unsorted notes, or an `unpacked_size` mismatch. The decoder should be tolerant; the encoder strictly mimics real copies.
- **[?]** Maximum track count and exact multi-track paste rules beyond §2.3.
- **[?]** The unnamed root/track chunks, including whether any of them depends on PPQ or on the clip's contents.

---

## 9. Planned future work (shape the architecture now)

- **Import `.dms`** (Domino project): per the chunk reference it uses **the same envelope and chunk grammar as the clipboard** (§2.1), so decoding needs no new codec: `DmsFileSource` = read file → `envelope::decode` → `Clip`. Differences to handle: a project root has more chunks than a clipboard copy (preserve them); `2009` may be absent (§2.6; synthesise it before writing to the clipboard); a project has many tracks, so before "Write to Domino clipboard" the user selects which tracks to send (Domino drops tracks beyond the last one). Because the file path through DominoKit is `.dms → Clip → flip/split → clipboard → user pastes into Domino`, no change to the clipboard rules is needed.
- **Import `.dmp`** (part file, magic `PortalPartData`): same envelope with a different magic [D]; content layout beyond the header is unconfirmed [?], so start by decoding it with the same chunk grammar and surface what's found in the Inspector before supporting it.
- **Import `.mid`** (Standard MIDI File → `Clip`): use `midly`; pair note-on/off into notes (`duration = off − on`, volume from note-on, note-on with velocity 0 = off); PPQ from the header's metrical timing (reject SMPTE timing); one clip track per MIDI track (or optionally per channel); set track `1001` channel and `1002` name from the MIDI data. Tracks are synthesised from Appendix A; set root `1002` PPQ and each track's `2009` length (choose "bar" or "note" stretch per §2.6). Other MIDI events map to the documented events (control → `2003`, program change → `2002`, text/lyric/marker/cue → `2005`/`2011`/`2017`/`2012`, sysex → `2004`), **except tempo (`2008`) and time signature (`2015`), which stay out until their encodings are verified (§8)**.
- All imports feed the same `Clip` model and therefore the existing flip/split/write pipeline; "send to running Domino" is simply **Write to Domino clipboard**, after which the user pastes. Define the entry point now:

```rust
pub trait ClipSource { fn load(&self) -> Result<Clip, SourceError>; }
// impls: ClipboardSource (v1), DmsFileSource, DmpFileSource, MidiFileSource (future)
```

- Possible later: export Clip → `.dms`/`.mid`; split by channel; carry-state-forward option for split; batch presets.

---

## Appendix A — Fallback templates from the reference implementation **[V]**

Use only when no source clip/track exists to clone from (e.g. the future imports). To customise a synthesised track (channel, name, port), parse `TRACK_HEAD` as chunks and replace the *value* of the relevant chunk (§2.3) rather than patching byte offsets. Hex, whitespace for readability only. Each constant is a **sequence of complete chunks** (see tables in §2.3). Assembly for a new clip:

```
unpacked = SONG_START
         + item(1002, ppq as u16 LE)
         + SONG_REST
         + for each track: item(1003, TRACK_HEAD + note_items + item(2009, item(1001, length as u32 LE)) + TRACK_TAIL)
         + SONG_TAIL
payload  = b"PortalSequenceData" + u32_le(len(unpacked)) + zlib(unpacked)
item(tag, body) = tag:u16 LE + len(body):u32 LE + body
```

```rust
use hex_literal::hex;

pub const MAGIC: &[u8; 18] = b"PortalSequenceData";

// 1000 (empty), 1001 copyright text (empty)
pub const SONG_START: &[u8] = &hex!(
    "e80300000000e90300000000");

// follows item 1002 (PPQ)
pub const SONG_REST: &[u8] = &hex!(
    "ef030400000030000000f1030400000000000000f40308000000000000000000"
    "0000f5030400000000000000f6030400000000000000fb0300000000fc030400"
    "000010000000fd030100000001fe030100000001ff0304000000110000000004"
    "0100000001");

pub const TRACK_HEAD: &[u8] = &hex!(
    "e803020000000000e9030100000000ea0300000000eb030100000000ec030100"
    "000000f003010000003cf1031100000047656e6572616c204d49444920447275"
    "6df303020000000000f4030400000000000000f8030400000064000000f90304"
    "00000000000000f5030100000001f603020000000100f7030100000001fa0301"
    "000000fffb030400000001000000fc0310000000000000000000000000000000"
    "00000000fd030100000000fe03010000007f");

pub const TRACK_TAIL: &[u8] = &hex!(
    "ed030400000032000000ee030100000064ef0304000000e0010000f2030e0000"
    "00e8030100000000e9030100000000");

pub const SONG_TAIL: &[u8] = &hex!(
    "ee0300000000f0031a000000e80300000000e9030400000001000000ea030400"
    "000001000000f903420000006400010000000065000100000000660001000000"
    "006700040000006400000068000100000000690004000000640000006a000c00"
    "0000050505050505050505050505");
```

Note construction (40 bytes): `item(2001, item(1001,u32 tick) + item(2001,u8 key) + item(2002,u8 vel) + item(2003,u32 gate))`.
Order notes by `(tick, key)` ascending. Tracks with no notes are not written by the reference (tracks are packed, no empty ones); one track per source channel/slot, in slot order.

## Appendix B — Reference reader behaviour worth porting (for Inspector/diagnostics only)

The reference reader (`read_notes`) walks every top-level `1003`, reads each track body as items, accepts runs of canonical 40-byte notes quickly and other-layout `2001` items individually, skips everything else, and returns rows `(tick, gate, key, velocity, track_index)` plus the PPQ — normalised as in §2.5. DominoKit's decoder is stricter about structure but must **not** adopt those normalisations for round-tripping.
