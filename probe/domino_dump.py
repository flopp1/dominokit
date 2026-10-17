"""Decode Domino chunk data into a readable tree.

Accepts: a DominoKit Inspector dump (raw unpacked chunks), a clipboard payload,
or a .dms/.dmp file (PortalSequenceData / PortalPartData envelope).

Usage:
    python domino_dump.py FILE [--tree]     # default: compact per-track event listing
    --tree   full chunk tree with every field and hex
"""
import struct
import sys
import zlib

MAGICS = (b"PortalSequenceData", b"PortalPartData")

ROOT = {1000: "name", 1001: "copyright", 1002: "PPQ", 1003: "TRACK"}
TRACK = {1000: "out port", 1001: "channel", 1002: "name", 1003: "?", 1004: "is rhythm",
         1005: "?", 1006: "volume", 1007: "gate", 1008: "?", 1009: "rhythm name",
         1010: "?container", 1011: "?", 1012: "tick adjust", 1013: "?", 1014: "?",
         1015: "enabled", 1016: "gate adjust", 1017: "transpose", 1018: "color",
         1019: "tick adj measure", 1020: "?", 1021: "range low", 1022: "range high",
         2009: "STRETCH LEN"}
EVENTS = {2001: "NOTE", 2002: "PROGRAM", 2003: "CONTROL", 2004: "SYSEX", 2005: "TEXT",
          2007: "EXPRESSION", 2008: "TEMPO", 2011: "LYRIC", 2012: "CUE", 2014: "MEASURE LINK",
          2015: "TIME SIG", 2016: "KEY SIG", 2017: "MARKER", 2018: "KEY SCALE", 2019: "CHORD"}
EVENT_FIELDS = {
    2001: {1001: "pos", 2001: "key", 2002: "vel", 2003: "gate"},
    2002: {1001: "pos", 2003: "patch"},
    2003: {1001: "pos", 2001: "cc", 2002: "data1", 2003: "data2"},
    2008: {1001: "pos", 2001: "value"},
    2015: {1001: "pos", 2001: "num", 2002: "denom"},
    2016: {1001: "pos", 2001: "key"},
}


def parse(buf):
    """Return (list of (id, body), leftover bytes)."""
    out, i = [], 0
    while i + 6 <= len(buf):
        cid, size = struct.unpack_from("<HI", buf, i)
        if i + 6 + size > len(buf):
            break
        out.append((cid, buf[i + 6:i + 6 + size]))
        i += 6 + size
    return out, buf[i:]


def clean(buf):
    """Body parses completely as chunks with plausible ids?"""
    if len(buf) < 6:
        return None
    items, rest = parse(buf)
    if rest or not items or any(c < 100 or c > 9999 for c, _ in items):
        return None
    return items


def val(b):
    n = len(b)
    if n == 0:
        return "(empty)"
    if n % 10 == 0 and all(b[i:i + 6] == b"\x00\x00\x04\x00\x00\x00" for i in range(0, n, 10)):
        # value list: chunk(s) id 0 holding an f32 (CC data, tempo)
        return "f32 " + ", ".join(f"{struct.unpack_from('<f', b, i + 6)[0]:g}" for i in range(0, n, 10))
    parts = []
    if n == 1:
        parts.append(f"{b[0]} (s8 {struct.unpack('<b', b)[0]})" if b[0] > 127 else str(b[0]))
    elif n == 2:
        u, s = struct.unpack("<H", b)[0], struct.unpack("<h", b)[0]
        parts.append(f"{u}" + (f" (s16 {s})" if s < 0 else ""))
    elif n == 4:
        u, s, f = struct.unpack("<I", b)[0], struct.unpack("<i", b)[0], struct.unpack("<f", b)[0]
        parts.append(f"{u}" + (f" (s32 {s})" if s < 0 else "") + (f" (f32 {f:g})" if 1e-6 < abs(f) < 1e7 else ""))
    elif n == 8:
        d = struct.unpack("<d", b)[0]
        parts.append(f"(f64 {d:g})" if 1e-6 < abs(d) < 1e9 else "")
    if n > 1 and all(32 <= c < 127 for c in b):
        parts.append(repr(b.decode("ascii")))
    elif n not in (1, 2, 4, 8) and any(c >= 128 for c in b):
        for enc in ("shift_jis", "utf-8", "cp1252"):
            try:
                parts.append(f"{enc}:{b.decode(enc)!r}")
                break
            except UnicodeDecodeError:
                pass
    parts.append(f"[{b.hex(' ')}]" if n <= 32 else f"[{b[:32].hex(' ')} …]")
    return " ".join(p for p in parts if p)


def tree(items, names, depth, scope):
    for cid, body in items:
        name = names.get(cid, "")
        if scope == "track" and cid >= 2001 and cid != 2009:
            name = "EVT " + EVENTS.get(cid, "UNKNOWN")
        sub = clean(body) if (scope != "event") else None
        pad = "  " * depth
        if sub is not None and (cid in (1003, 2009) or scope == "track" and cid >= 2001 or len(body) >= 6):
            print(f"{pad}{cid} (0x{cid:04x}) {name}  [{len(body)} bytes, container]")
            if scope == "root" and cid == 1003:
                tree(sub, TRACK, depth + 1, "track")
            elif scope == "track" and cid >= 2001:
                tree(sub, EVENT_FIELDS.get(cid, {1001: "pos"}), depth + 1, "event")
            else:
                tree(sub, {}, depth + 1, "other")
        else:
            print(f"{pad}{cid} (0x{cid:04x}) {name}: {val(body)}")


def compact(root):
    for cid, body in root:
        if cid != 1003:
            sub = clean(body)
            if sub and len(body) >= 6:
                print(f"root {cid}: {{ " + ", ".join(f"{c}={val(b)}" for c, b in sub) + " }")
            else:
                print(f"root {cid} {ROOT.get(cid, '')}: {val(body)}")
            continue
        items, rest = parse(body)
        print(f"\n=== TRACK ===")
        settings = []
        for tid, tb in items:
            if tid < 2001:
                sub = clean(tb)
                shown = "{ " + ", ".join(f"{c}={val(b)}" for c, b in sub) + " }" if sub else val(tb)
                settings.append(f"{tid}{'/' + TRACK[tid] if tid in TRACK and TRACK[tid] != '?' else ''}={shown}")
        print("  settings: " + " | ".join(settings))
        for tid, tb in items:
            if tid < 2001:
                continue
            sub = clean(tb)
            label = TRACK.get(tid) if tid == 2009 else EVENTS.get(tid, "UNKNOWN")
            if sub is None:
                print(f"  {tid} {label}: (not chunked) {val(tb)}")
                continue
            fields = EVENT_FIELDS.get(tid, {1001: "pos"})
            d = dict(sub)
            pos = struct.unpack("<I", d[1001])[0] if len(d.get(1001, b"")) == 4 else None
            rest_f = ", ".join(f"{c}{'/' + fields[c] if c in fields else ''}={val(b)}"
                               for c, b in sub if c != 1001)
            print(f"  @{pos!s:>6}  {tid} {label:<10} {rest_f}")
        if rest:
            print(f"  TRAILING {len(rest)} bytes: {rest.hex(' ')}")


def load(path):
    data = open(path, "rb").read()
    for m in MAGICS:
        if data.startswith(m):
            size = struct.unpack_from("<I", data, len(m))[0]
            out = zlib.decompressobj().decompress(data[len(m) + 4:])
            print(f"# {m.decode()} envelope, unpacked_size={size}, got {len(out)}")
            return out
    print("# raw unpacked chunk data")
    return data


def main():
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    data = load(sys.argv[1])
    root, rest = parse(data)
    if "--tree" in sys.argv:
        tree(root, ROOT, 0, "root")
    else:
        compact(root)
    if rest:
        print(f"ROOT TRAILING {len(rest)} bytes: {rest.hex(' ')}")


if __name__ == "__main__":
    main()
