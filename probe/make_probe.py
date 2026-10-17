"""Generate probe.mid: a Standard MIDI File exercising every MIDI event type
(except sysex), laid out so each event in Domino's data can be identified by
its track and tick alone.  Also writes PROBE_MANIFEST.md listing every event.

Format 1, PPQ 480, one channel per track (Domino keeps one channel per track).
Usage:  python make_probe.py [outdir]
"""
import os
import sys

PPQ = 480
BAR = 4 * PPQ  # 4/4 everywhere except the time-signature section at the end


def vlq(n):
    out = [n & 0x7F]
    n >>= 7
    while n:
        out.append(0x80 | (n & 0x7F))
        n >>= 7
    return bytes(reversed(out))


def meta(t, data):
    return bytes([0xFF, t]) + vlq(len(data)) + data


class Track:
    def __init__(self, name, ch=None):
        self.name = name
        self.ch = ch
        self.ev = []  # (tick, seq, bytes, description)
        self.add(0, meta(0x03, name.encode("ascii")), f"track name '{name}'")

    def add(self, tick, data, desc, show=True):
        self.ev.append((tick, len(self.ev), bytes(data), desc if show else None))

    # channel voice helpers --------------------------------------------------
    def note(self, tick, key, vel, gate, off_vel=0x40, off_as_note_on=False):
        c = self.ch
        self.add(tick, [0x90 | c, key, vel],
                 f"NOTE key={key} vel={vel} gate={gate}"
                 + (" (off = note-on vel 0)" if off_as_note_on else f" (off vel {off_vel})"))
        off = [0x90 | c, key, 0] if off_as_note_on else [0x80 | c, key, off_vel]
        self.add(tick + gate, off, None, show=False)

    def cc(self, tick, num, val, desc=""):
        self.add(tick, [0xB0 | self.ch, num, val], f"CC#{num} = {val}" + (f"  {desc}" if desc else ""))

    def pc(self, tick, prog):
        self.add(tick, [0xC0 | self.ch, prog], f"PROGRAM {prog} (0-based; Domino may show {prog + 1})")

    def chan_pressure(self, tick, val):
        self.add(tick, [0xD0 | self.ch, val], f"CHANNEL PRESSURE {val}")

    def poly_pressure(self, tick, key, val):
        self.add(tick, [0xA0 | self.ch, key, val], f"POLY PRESSURE key={key} val={val}")

    def bend(self, tick, raw):
        self.add(tick, [0xE0 | self.ch, raw & 0x7F, raw >> 7],
                 f"PITCH BEND raw={raw} (signed {raw - 8192})")

    def encode(self):
        body = bytearray()
        last = 0
        for tick, _, data, _ in sorted(self.ev, key=lambda e: (e[0], e[1])):
            body += vlq(tick - last) + data
            last = tick
        body += vlq(0) + meta(0x2F, b"")
        return b"MTrk" + len(body).to_bytes(4, "big") + bytes(body)


def tempo(bpm_or_us, is_us=False):
    us = bpm_or_us if is_us else round(60_000_000 / bpm_or_us)
    return meta(0x51, us.to_bytes(3, "big")), us


def build():
    tracks = []

    # --- 0: conductor -------------------------------------------------------
    t = Track("Conductor")
    t.add(0, meta(0x00, b"\x00\x07"), "SEQUENCE NUMBER 7")
    t.add(0, meta(0x02, b"Probe copyright"), "COPYRIGHT 'Probe copyright'")
    t.add(0, meta(0x54, bytes([0x21, 0x02, 0x03, 0x04, 0x05])),
          "SMPTE OFFSET hr=0x21(25fps,1h) mn=2 se=3 fr=4 ff=5")
    for tick, bpm in [(0, 120), (480, 123.456), (BAR, 133.333), (2 * BAR, 90.5),
                      (3 * BAR, 300), (4 * BAR, 60), (5 * BAR, 120)]:
        data, us = tempo(bpm)
        t.add(tick, data, f"TEMPO {bpm} BPM ({us} us/qn)")
    data, us = tempo(8_000_000, is_us=True)
    t.add(5 * BAR + 960, data, f"TEMPO 7.5 BPM ({us} us/qn, slow extreme)")
    t.add(0, meta(0x58, bytes([4, 2, 24, 8])), "TIME SIG 4/4 (dd=2, cc=24, bb=8)")
    for tick, sf, mi, name in [(0, 0, 0, "C major"), (BAR, 1, 0, "G major"),
                               (2 * BAR, -6, 1, "Eb minor"), (3 * BAR, 6, 0, "F# major"),
                               (4 * BAR, -7, 0, "Cb major"), (5 * BAR, 0, 1, "A minor"),
                               (5 * BAR + 480, 7, 1, "A# minor"), (5 * BAR + 1440, -1, 0, "F major")]:
        t.add(tick, meta(0x59, bytes([sf & 0xFF, mi])), f"KEY SIG sf={sf} mi={mi} ({name})")
    base = 6 * BAR  # 11520
    texts = [
        (0x01, b"TEXT-ascii", "TEXT 'TEXT-ascii'"),
        (0x05, b"LYRIC-ascii", "LYRIC 'LYRIC-ascii'"),
        (0x06, b"MARKER-ascii", "MARKER 'MARKER-ascii'"),
        (0x07, b"CUE-ascii", "CUE POINT 'CUE-ascii'"),
        (0x04, b"INSTR-name", "INSTRUMENT NAME 'INSTR-name'"),
        (0x08, b"PROGNAME", "PROGRAM NAME 'PROGNAME'"),
        (0x09, b"DEVNAME", "DEVICE NAME 'DEVNAME'"),
        (0x7F, bytes([0x00, 0x00, 0x41, 0x12, 0x34]), "SEQUENCER-SPECIFIC 00 00 41 12 34"),
        (0x01, "テスト".encode("shift_jis"), "TEXT Shift-JIS 'テスト' (83 65 83 58 83 67)"),
        (0x01, "café".encode("latin-1"), "TEXT Latin-1 'café' (63 61 66 E9)"),
        (0x01, "日本".encode("utf-8"), "TEXT UTF-8 '日本' (E6 97 A5 E6 9C AC)"),
        (0x06, "マーカー".encode("shift_jis"), "MARKER Shift-JIS 'マーカー'"),
        (0x01, b"", "TEXT empty"),
    ]
    for i, (mt, data, desc) in enumerate(texts):
        t.add(base + i * 120, meta(mt, data), desc)
    # time-signature section: each change on its own bar line
    tick = 8 * BAR  # 15360
    for num, dd, cc, name in [(3, 2, 24, "3/4"), (6, 3, 12, "6/8"), (7, 4, 6, "7/16"),
                              (5, 1, 48, "5/2"), (13, 3, 12, "13/8"), (4, 2, 24, "4/4")]:
        t.add(tick, meta(0x58, bytes([num, dd, cc, 8])), f"TIME SIG {name} (dd={dd}, cc={cc})")
        tick += num * (4 * PPQ // (1 << dd))
    tracks.append(t)

    # --- 1: notes, ch 1 -----------------------------------------------------
    t = Track("Notes ch1", 0)
    t.note(0, 60, 100, 480)
    t.note(480, 0, 1, 1)
    t.note(960, 127, 127, 1920)
    for k, v, g in [(60, 80, 240), (64, 81, 480), (67, 82, 960)]:
        t.note(BAR, k, v, g)
    t.note(2 * BAR, 62, 90, 100, off_as_note_on=True)
    t.note(2 * BAR + 480, 64, 70, 333, off_vel=127)
    t.note(3 * BAR, 65, 50, 960)
    t.note(3 * BAR + 240, 66, 51, 240)
    t.note(4 * BAR, 72, 99, 65537)  # gate > 16 bits
    tracks.append(t)

    # --- 2: every plain CC, ch 1; tick = cc * 120 ---------------------------
    special = {0, 32, 6, 38, 96, 97, 98, 99, 100, 101}
    t = Track("CC plain ch1", 0)
    for n in range(128):
        if n not in special:
            t.cc(n * 120, n, (n * 37 + 11) % 128, "(tick = cc*120)")
    for i, v in enumerate([0, 1, 63, 64, 126, 127]):
        t.cc(128 * 120 + i * 120, 11, v, "(range test)")
    tracks.append(t)

    # --- 3: bank select / program, ch 2 -------------------------------------
    t = Track("Bank Program ch2", 1)
    t.pc(0, 5)
    t.cc(480, 0, 0x12, "bank MSB"); t.cc(480, 32, 0x34, "bank LSB"); t.pc(480, 0x56)
    t.cc(960, 0, 0x21, "bank MSB only"); t.pc(960, 7)
    t.cc(1440, 32, 0x43, "bank LSB only"); t.pc(1440, 8)
    t.pc(1920, 0)
    t.pc(2400, 127)
    t.cc(2880, 0, 0x79, "bank MSB alone, no program")
    t.cc(3360, 32, 0x0A, "bank LSB alone, no program")
    t.cc(3840, 0, 1, "bank MSB, separate ticks"); t.cc(3900, 32, 2, "bank LSB, separate ticks"); t.pc(3960, 3)
    tracks.append(t)

    # --- 4: RPN / NRPN / data entry, ch 3 -----------------------------------
    t = Track("RPN NRPN ch3", 2)
    seqs = [
        (0, [(101, 0), (100, 0), (6, 12), (38, 0)], "RPN 0/0 pitch-bend sens = 12/0"),
        (480, [(101, 0), (100, 1), (6, 0x40), (38, 0x01)], "RPN 0/1 fine tune 64/1"),
        (960, [(101, 0), (100, 2), (6, 0x46)], "RPN 0/2 coarse tune 70, MSB only"),
        (1440, [(99, 1), (98, 8), (6, 0x50)], "NRPN 1/8 = 80, MSB only"),
        (1920, [(99, 0x12), (98, 0x34), (6, 0x56), (38, 0x78)], "NRPN 18/52 = 86/120"),
        (2400, [(101, 127), (100, 127)], "RPN null"),
        (2880, [(101, 0), (100, 0), (96, 0)], "RPN 0/0 + data increment"),
        (3360, [(97, 0)], "data decrement alone"),
        (3840, [(6, 0x33)], "data entry MSB alone"),
        (4320, [(38, 0x44)], "data entry LSB alone"),
        (4800, [(101, 0x05)], "RPN MSB alone"),
        (5280, [(100, 0x06)], "RPN LSB alone"),
        (5760, [(99, 0x07)], "NRPN MSB alone"),
        (6240, [(98, 0x08)], "NRPN LSB alone"),
    ]
    for tick, msgs, desc in seqs:
        for n, v in msgs:
            t.cc(tick, n, v, f"[{desc}]")
    tracks.append(t)

    # --- 5: pitch bend, ch 4 ------------------------------------------------
    t = Track("Pitch bend ch4", 3)
    for i, raw in enumerate([0, 8192, 16383, 1, 0x1234, 8193]):
        t.bend(i * 480, raw)
    tracks.append(t)

    # --- 6: aftertouch, ch 5 ------------------------------------------------
    t = Track("Aftertouch ch5", 4)
    for k in (60, 61, 127):
        t.note(0, k, 100, BAR)
    for i, (k, v) in enumerate([(60, 99), (61, 0), (127, 127), (60, 1)]):
        t.poly_pressure(120 + i * 120, k, v)
    for i, v in enumerate([0, 64, 127, 33]):
        t.chan_pressure(960 + i * 120, v)
    tracks.append(t)

    # --- 7: drums, ch 10 ----------------------------------------------------
    t = Track("Drums ch10", 9)
    t.add(0, meta(0x04, b"DrumKit"), "INSTRUMENT NAME 'DrumKit'")
    for i, k in enumerate([36, 38, 42]):
        t.note(i * 240, k, 110 - i, 120)
    tracks.append(t)

    # --- 8: channel 16, port prefix 1 ---------------------------------------
    t = Track("Ch16 Port2", 15)
    t.add(0, meta(0x21, b"\x01"), "MIDI PORT prefix = 1 (second port)")
    t.add(0, meta(0x20, b"\x0F"), "MIDI CHANNEL prefix = 15")
    t.note(0, 72, 64, 480)
    tracks.append(t)

    return tracks


def main():
    outdir = sys.argv[1] if len(sys.argv) > 1 else os.path.dirname(os.path.abspath(__file__))
    tracks = build()
    smf = b"MThd" + (6).to_bytes(4, "big") + (1).to_bytes(2, "big") \
        + len(tracks).to_bytes(2, "big") + PPQ.to_bytes(2, "big")
    smf += b"".join(t.encode() for t in tracks)
    with open(os.path.join(outdir, "probe.mid"), "wb") as f:
        f.write(smf)

    lines = ["# probe.mid manifest", "",
             f"Format 1, PPQ {PPQ}, {len(tracks)} tracks. Ticks are absolute from song start. "
             "Note-offs are omitted (implied by gate). Raw bytes are the SMF event bytes.", ""]
    for i, t in enumerate(tracks):
        ch = "—" if t.ch is None else f"{t.ch + 1} (0-based {t.ch})"
        lines += [f"## Track {i}: {t.name}  · channel {ch}", "",
                  "| tick | raw bytes | event |", "|---:|---|---|"]
        for tick, _, data, desc in sorted(t.ev, key=lambda e: (e[0], e[1])):
            if desc:
                lines.append(f"| {tick} | `{data.hex(' ')}` | {desc} |")
        lines.append("")
    with open(os.path.join(outdir, "PROBE_MANIFEST.md"), "w", encoding="utf-8") as f:
        f.write("\n".join(lines))
    print(f"wrote probe.mid ({len(smf)} bytes) and PROBE_MANIFEST.md to {outdir}")


if __name__ == "__main__":
    main()
