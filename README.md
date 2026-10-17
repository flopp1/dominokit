# DominoKit

A small desktop utility that sits beside [Domino](https://www3.nsknet.or.jp/~kkgr/soft/infano.htm)
(the Windows MIDI sequencer). It flips and chops the notes you copy out of
Domino, and moves note data between Domino's clipboard, `.dms` project files
and `.mid` files.

DominoKit never touches Domino itself: you copy, DominoKit transforms, you paste.

```
In Domino:     select notes, Ctrl+C
In DominoKit:  Read (clipboard → working copy)
               Flip and/or Chop          (each one undoable)
               Write (working copy → clipboard)
In Domino:     highlight the target track, Ctrl+V
```

## Window

- **Toolbar** — *Clipboard: Read / Write*, *Files: Open… / Save as ▾*, and
  *Clipboard ↔ file ▾* for direct transfers. *Skip conductor track when
  writing* (on by default) leaves a project's conductor track out of what is
  written to the clipboard.
- **Flip** and **Chop** cards on the left. Both always apply to the whole
  working copy: every track, every note.
- **Overview** — read-only picture of all tracks' notes (shaded by
  velocity), with the flip range (blue band, dashed mirror axis) and the chop
  grid drawn as a preview, and a track list underneath. It opens on the
  first **7680 ticks**; zoom with **Ctrl + wheel** or the − / + buttons,
  scroll with the wheel, by dragging or with the scroll bar; *7680* resets
  the zoom, *Whole clip* shows everything.
- **Inspector** — the decoded chunks of the song or of one track, one line
  per chunk, paged for huge tracks.
- Status line at the bottom. Undo (Ctrl+Z) and Clear in the header.

Every action runs on a background thread, so the window never freezes: while
one runs the controls are locked and, if it takes more than a second, a
progress window shows what it is doing.

Shortcuts (only while DominoKit has focus): Ctrl+R read, Ctrl+W write,
Ctrl+O open, Ctrl+Z undo. A `.dms`/`.mid` dropped on the window, or passed on
the command line, is opened.

## Flip

Mirrors every note whose key is between **From key** and **To key**
(inclusive): the two ends swap, the next two inward swap, and so on. Notes
outside the range don't move. Keys are numbered **1 to 256** (key `n` = key
byte `n − 1`), so keys 1–128 are MIDI notes 0–127 and flipping 1–128 is the
classic `127 − k`. Only note keys change; everything else stays byte-for-byte.

## Chop

Cuts every note at a grid that starts at tick 0 of the clip (the start of the
copied range) and repeats every **N ticks**, in every track. A note crossing
grid lines becomes one note per grid cell, with the same key and velocity.

The tick box takes whole ticks (`120`) or a fraction (`100/3`). Underneath,
pick a **PPQ** and a **note length** (1/1 … 1/64, plain / dotted / triplet) and
the length in ticks is written straight into the box. The PPQ follows the
loaded clip. Fractional lengths (e.g. 1/8 triplet at PPQ 100) are rounded per
grid line, so they don't drift.

## Clipboard and files

| Action | From | To |
|---|---|---|
| Read | clipboard | working copy |
| Write | working copy | clipboard (replaces its contents) |
| Open… | `.dms` / `.mid` | working copy |
| Save as ▾ | working copy | `.dms` or `.mid` |
| Clipboard ↔ file ▾ → Save the clipboard as… | clipboard | `.dms` or `.mid` (working copy untouched) |
| Clipboard ↔ file ▾ → Put a file on the clipboard… | `.dms` / `.mid` | clipboard (working copy untouched) |

- A clipboard copy has no conductor track; saving one as `.dms`/`.mid` adds an
  empty conductor track first, so every copied track opens as a normal track.
  Writing a project back to the clipboard leaves the conductor out again
  (unless the toolbar option is off), and gives every track the same length
  as a real Domino copy has.
- MIDI conversion follows Domino's own import/export exactly (checked against
  Domino's files in `probe/`): CC/pitch bend/channel pressure become Control
  Change Macros using Domino's GMLevel1 table (pan is stored −64…63), program
  numbers are stored +1, tempo is a BPM float, note gates are capped at 65535
  on import as Domino does. Things a MIDI file can't hold (measure links,
  chords, scales, variables, sysex) are listed in the status line on export.
  Key signatures are written at their real ticks (Domino's own export moves
  them to the start of their measure).
- DominoKit only ever exchanges Domino's registered clipboard format
  `MidiPortalSequence`, only when you press Read / Write (or a transfer), and
  never simulates keystrokes or touches Domino's process.

## Performance

Every stage processes tens to hundreds of millions of notes per second on a
12-core machine (1 M notes in 16 tracks; `cargo run --release --example bench`):

| Stage | M notes/s |
|---|---|
| Decode / encode the chunk data | ~230 / ~240 |
| Read a payload (zlib + decode) — Domino's own / DominoKit's | ~15 / ~60 |
| Build a clipboard payload (encode + check + zlib) | ~40 |
| Flip | ~1300 (in place) |
| Chop (1/4 → 1/16, 5× the notes out) | ~30 in, ~160 out |
| `.mid` import / export | ~150 / ~100 |

How: a note is a 12-byte value (16 bytes in a track), with no heap
allocation; Domino's canonical 40-byte note layout is pattern-matched
straight out of the bytes; tracks — and big tracks in verified pieces — are
decoded, encoded, flipped and chopped on all cores; chop produces its notes
already in (tick, key) order instead of sorting; zlib runs on `zlib-rs`,
compressing in parallel blocks (which DominoKit can also decompress in
parallel; Domino's own streams decompress sequentially); MIDI is streamed
without intermediate event lists. Nothing copies the working copy: writes
encode straight from it, Flip is undone by flipping again, and Chop's undo
keeps the clip it replaced within a 512 MB budget.

The overview rasterises notes into one texture, in parallel stripes, only
when the view or the clip changes. Overlapping notes are removed while
rasterising — per track and key, a note hidden behind what is already drawn
costs one comparison and no pixels — and notes are indexed by length class
so a time window is found by binary search. 5 M notes all in view redraw in
~14 ms; the default 7680-tick view in ~1 ms. `dominokit --stress 5000000 16`
loads 5 M random notes to try it.

## Limits

There is no cap on note counts in DominoKit: chop, import and the overview
take as many notes as memory holds (a chop that can't get the memory says so
instead of crashing). The only limits are the file formats': Domino stores
each chunk's size and the whole clip's size in 32 bits (at most 4 GiB in
all, about 107 million notes per track), and a MIDI file holds at most
65535 tracks of up to 4 GiB each. Writing something bigger is refused with
an explanation rather than producing a broken file.

## Building

Rust (stable, edition 2024). Windows is the primary platform; elsewhere the
GUI builds and runs with the clipboard disabled.

```
cargo build --release      # binary: target/release/dominokit.exe
cargo test                 # includes round trips of the real files in probe/
dominokit --demo           # explore the UI with a synthetic 3-track clip
dominokit --stress N [T]   # N random notes in T tracks
cargo run --release -p dominokit-core --example bench -- 1000000 16
cargo run --release -p dominokit-core --example chopbench -- 10000000 1 120
```

## Things to keep in mind

- **Domino does not convert PPQ on paste.** Make sure the destination song
  uses the clip's PPQ (shown in the header).
- The first copied track pastes into the *highlighted* track and the rest
  into the tracks below; tracks beyond the last existing one are dropped.
- The format reference is `probe/FINDINGS.md` (verified against real Domino
  files) on top of `DOMINOKIT_SPEC.md` (the original brief).

## Layout

```
crates/
  dominokit-core/   pure logic, no GUI / OS APIs
    format/         envelope, chunks, clip model (lossless), schema, event
                    decoding, SMF codec, MIDI ↔ clip, .dms / clipboard I/O
    ops/            flip (key range), chop (tick grid)
  dominokit/        eframe/egui app + Win32 clipboard backend
```
