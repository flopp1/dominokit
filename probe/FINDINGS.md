# Probe findings: Domino `.dms` ↔ MIDI (round 1)

Sources: `probe.mid` (generated, see `PROBE_MANIFEST.md`) → imported into Domino →
`probe.dms` (saved) and `probe_out.mid` (Domino's own SMF export), plus screenshots
of Domino's event list. **[V]** = verified from these files.

## 1. Envelope and root

- `.dms` = `PortalSequenceData` + u32 unpacked size + zlib: identical to the clipboard envelope **[V]**.
- Root `1000` (song name) = the **first SMF track's name**; root `1001` = the copyright meta (FF 02) **[V]**. On export both are written back to track 0 as FF 03 / FF 02.
- Root `1002` = PPQ (480) **[V]**.
- Root `1013` = 15 here (0 in the clipboard template). Meaning unknown **[?]**. Every other root chunk is identical to the clipboard template.
- One `1003` track per SMF track, in SMF order. SMF track 0 becomes Domino's **conductor track** **[V]**.

## 2. Track settings

| ID | Finding |
|---|---|
| 1000 out port | u16, 0 = PortA, 1 = PortB. Set from the FF 21 port prefix **[V]**. Exported as `FF 21 01 pp` on every non-conductor track. |
| 1001 channel | u8, 0-based, from the channel of the track's events **[V]** |
| 1002 name | Raw bytes from FF 03 **[V]** |
| 1003 | **1 = conductor track** (only track 0 has 1) **[V]** |
| 1004 is rhythm | 1 for the channel-10 track **[V]** |
| 2009 | **End of Track position** (absolute tick of the FF 2F event), not specific to the clipboard. Exported as FF 2F at that tick **[V]** |

## 3. Event encodings

### Value encoding: the "f32 list"
Value fields of CONTROL (2003) and TEMPO (2008) are **not plain numbers**. Each one is a chunk sequence holding one chunk with **id 0, size 4, f32 LE**:
`00 00 04 00 00 00 <f32>` **[V]**. Having a list suggests a gradual change (start→end) could hold more than one entry **[I]**; this needs testing (§6).

### Notes (2001)
- Same layout as documented. Note-off velocity is discarded; a note-on with velocity 0 is accepted as a note-off **[V]**.
- **Gate is capped at 65535** on import (65537 → 65535) even though the field is u32 **[V]**. Export writes `8n kk 00`.

### Tempo (2008)
- `2001` = f32 list holding **BPM**, e.g. 123.456 → f32 123.456; 450001 µs → 133.333 **[V]**.
- Export: µs = round(60 000 000 / BPM) (90.50006 → 662983 exactly) **[V]**.

### Time signature (2015)
- `2001` num u8, `2002` denom u8 **literal** (4, 8, 16, 2) **[V]**.
- The SMF cc/bb bytes are discarded; export always writes `cc=24, bb=8` **[V]**.

### Key signature (2016): `2001` u8
`major: sharps n → n (0–7); flats n → 8 + n (F=9 … Cb=15); minor: same + 16` **[V]**

| Key | Stored |
|---|---|
| C | 0 |
| G | 1 |
| F# | 6 |
| F | 9 |
| Cb | 15 |
| A minor | 16 |
| A# minor | 23 |
| Eb minor | 30 |

On export Domino writes **one key signature per measure, at the measure start**, and the last one in the measure wins: the A minor, A# minor and F major in bar 6 became a single `FF 59 ff 00` (F major) at 9600 **[V]**. The `.dms` itself keeps all three at their real ticks.

### Text events: `2001` = raw bytes, no transcoding **[V]**
| SMF meta | Domino event |
|---|---|
| FF 01 text | 2005 |
| FF 05 lyric | 2011 |
| FF 06 marker | 2017 |
| FF 07 cue | 2012 |

Shift-JIS, Latin-1 and UTF-8 bytes are all stored and exported byte-identical. Domino *displays* them as CP932.

### Program change (2002)
- `2003` patch = **MIDI program + 1** (1..128) **[V]**.
- `2001` s8 = −1, `2002` s8 = −1, `2004` empty, `2005` u8 0, `2006` s16 −1 in every case. 2001 and 2002 are probably bank MSB/LSB (−1 = none) **[I]**.
- Bank select CC0/CC32 stay **separate CONTROL events**, even at the same tick as the program change **[V]**.

### Control (2003) = an instance of a module "CCM" (Control Change Macro)
`2001` u16 = **CCM ID** · `2002` = the CCM's **Gate** parameter (f32 list) · `2003` = the CCM's **Value** parameter (f32 list) **[V]**

The meaning of a CONTROL event is **defined by the module XML** (`Domino\Module\*.xml`, `<ControlChangeMacroList>`), not fixed by the file format:

```xml
<CCM ID="10" Name="Part Panpot"> <Value Min="-64" Max="63" Offset="64" /> <Data>@CC 10 #VL</Data> </CCM>
<CCM ID="130" Name="PitchBend">  <Value Min="-8192" Max="8191" Offset="8192" /> <Data>@PB #VH #VL</Data> </CCM>
<CCM ID="129" Name="129 PAT">    <Value /> <Gate Type="Key" ... /> <Data>@PKP #GL #VL</Data> </CCM>   (XG modules)
```

- Stored value = MIDI value − `Value/@Offset`. Stored gate = MIDI gate − `Gate/@Offset` (both default to 0) **[V for Value]**.
- `<Data>` is a small template language. Its commands are `@CC n`, `@PB`, `@CP`, `@PKP`, `@RPN msb lsb`, `@NRPN msb lsb` and `@SYSEX … [ checksummed … ]`. Its operands are `#VL`/`#VH` (value low/high 7 bits), `#GL` (gate), `#NONE` (omit the LSB data entry), `#VF1..4` (value nibbles), `#1RCH`/`#2RCH`/`#4RCH` (Roland checksum variants?), and literals like `40H` (hex) or `24` (decimal).
- The ID ranges seen across the 6 modules:
  - 0–127 = plain CC.
  - 128 = "Rest" (no output).
  - 129 = poly pressure (XG/GS modules only).
  - 130 = pitch bend.
  - 132 = channel pressure.
  - 140+ = RPN/NRPN/sysex macros. For example, GM's 140 = `@RPN 00H 00H #VL #NONE` (pitch-bend range).
- **The module isn't stored in the .dms** as far as we can tell. Root 1008 and 1017 hold no name; it is an application setting (GMLevel1 for all ports here). **The converter must take a module XML as input**, defaulting to a built-in GMLevel1 table.

**Domino's own MIDI import is simple** **[V]**:

| MIDI event | Stored as |
|---|---|
| CC n | id n (offset from CCM n, if the module defines it) |
| `En` pitch bend | id 130 |
| `Dn` channel pressure | id 132 |
| `An` poly pressure | the module's `@PKP` CCM; **dropped if the module has none**, which is why GMLevel1 lost it |

RPN/NRPN are never folded into macros: every CC 101/100/99/98/6/38/96/97 stays its own event, in order at equal ticks.

**Domino's export** expands each CONTROL event through its CCM `<Data>` template. For a macro such as 140, that means several CC messages.

**Gradual changes are not a stored event type** **[V]**. Domino's "CC/Tempo Value" adjustments and line/curve tools rewrite ordinary single events: tempo 120 → 120.05, CC7 14 → 15, and pitch bends −8192 → −8189 and so on. Every value list seen so far has exactly one entry.

### Domino-only events (inserted by hand on the conductor track, tick 0)

| ID | Event (Insert menu) | Fields seen | In SMF export |
|---|---|---|---|
| 2014 | Measure Link | 2001 u32 = 1 (dest measure), 2002 s32 = 0 | dropped |
| 2007 | Variables | 2001 string (empty), 2002 string (empty) | dropped |
| 2012 | Cue Point | 2001 string (empty) | `FF 07` |
| 2005 | **Comment** (= text) | 2001 'Text' | `FF 01` |
| 2019 | Chord "C Maj" | 2001 u8 = 0 (root), 2002 **u32** = 100 (chord type id) | dropped |
| 2018 | Scale "C Natural Major" | 2001 u8 = 0 (key), 2002 **u32** = 100 (scale type id) | dropped |

These are fewer and different from the chunk reference's field lists (no 2003/2004), so decode them by actual size.

### Tempo export rounding
µs = **trunc(f32(60 000 000 / bpm))**, computed in single precision. This matches all 5 non-integer cases exactly; plain rounding misses 120.05 → 499791 **[V]**.

## 3b. Clipboard copy (bars 2–9 of five tracks; `clip1.bin`) **[V]**

- Positions are **relative to the start of the copy range** (bar 2 → 0). Only events whose start lies inside the range are copied. The key-127 note that started in bar 1 and sounded into bar 2 was not.
- Every copied track gets `2009` = range length (14880 = bar 2 up to bar 10, through the 3/4 bar), including tracks with no events in range.
- **Track settings are not copied**: every track has template settings (channel 0, empty name, port 0). Root `1001` copyright = Domino's default `'Copyright (C) 2026'`, and root 1013 = 0.
- The conductor track was not in the selection and is not in the clip.

## 4. Dropped on import (no Domino equivalent) **[V]**

- FF 00 sequence number
- FF 54 SMPTE offset
- FF 04 instrument name
- FF 08 program name
- FF 09 device name
- FF 7F sequencer-specific
- FF 20 channel prefix
- **An A0 poly key pressure event**: only because GMLevel1 has no `@PKP` CCM

FF 21 is converted to the track port. Sysex is out of scope.

## 5. Domino's SMF export format **[V]**

- Format 1, the same track count, the same PPQ, no running status.
- Note-on `9n`, note-off `8n kk 00`.
- Track 0: name, copyright, then conductor events.
- Other tracks: `FF 03 name`, `FF 21 port`, then events, then `FF 2F` at the 2009 tick.
- Events at the same tick keep their original order.

## 6. Still unknown (minor; none blocks the converter)

1. Root `1013`: 15 after import, 1129 after editing, 0 in clips. Looks like editor/UI state; preserve it, and write 0 when synthesising.
2. Program-change fields `2001`/`2002`/`2006` when a bank is set in Domino's program-change dialog (probably bank MSB/LSB, and output as CC0/CC32 on export).
3. The "Multiple Control Changes" insert (`S`/`G`): one event, or many?
4. Whether a CC value can be non-integer (from curve tools), and how export rounds it. Assume trunc, as for tempo.
5. The Gate offset on a gated CCM (for example PKP in a GS/XG module) has not been observed yet.
6. Undocumented event ids 2006/2010/2013: none seen so far.
