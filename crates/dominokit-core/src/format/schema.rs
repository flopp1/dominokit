//! Data-driven schema of the chunk format, as verified against real Domino
//! `.dms` files and clipboard copies (`probe/FINDINGS.md`).
//!
//! IDs are scoped by their parent, so there are three lookup tables:
//! root chunks, track chunks, and the fields inside each event container.
//! Unknown paths fall back to `None`; callers keep their bytes verbatim.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    U8,
    S8,
    U16,
    S16,
    U32,
    S32,
    /// Raw bytes, no terminator, no length prefix (displayed as CP932 by Domino).
    String,
    /// Opaque bytes.
    Raw,
    /// A chunk sequence.
    Container,
    /// A chunk sequence of `id 0, size 4, f32 LE` entries (CONTROL / TEMPO values).
    F32List,
    /// Empty chunk (presence only).
    Empty,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Field {
    pub name: &'static str,
    pub kind: FieldKind,
}

const fn f(name: &'static str, kind: FieldKind) -> Option<Field> {
    Some(Field { name, kind })
}

use FieldKind::*;

/// Song-level (root) chunks.
pub fn root_field(id: u16) -> Option<Field> {
    match id {
        1000 => f("song name", String),
        1001 => f("copyright", String),
        1002 => f("PPQ", U16),
        1003 => f("track", Container),
        1006 => f("(unknown)", Empty),
        1007 => f("(unknown)", U32),
        1008 => f("(unknown)", Container),
        1009 => f("(unknown)", U32),
        1012 => f("(unknown)", Raw),
        1013 => f("(editor state?)", U32),
        1014 => f("(unknown)", U32),
        1017 => f("(unknown)", Container),
        1019 => f("(unknown)", Empty),
        1020 => f("(unknown)", U32),
        1021 => f("(unknown)", U8),
        1022 => f("(unknown)", U8),
        1023 => f("(unknown)", U32),
        1024 => f("(unknown)", U8),
        _ => None,
    }
}

/// Chunks directly inside a track (`1003`) that are not events.
pub fn track_field(id: u16) -> Option<Field> {
    match id {
        1000 => f("out port", U16),
        1001 => f("channel (0-based)", U8),
        1002 => f("track name", String),
        1003 => f("conductor track flag", U8),
        1004 => f("rhythm track flag", U8),
        1005 => f("(unknown)", U32),
        1006 => f("volume", U8),
        1007 => f("gate", S32),
        1008 => f("(unknown)", U8),
        1009 => f("rhythm name", String),
        1010 => f("(unknown)", Container),
        1011 => f("(unknown)", U16),
        1012 => f("tick adjust", U32),
        1013 => f("(unknown)", U8),
        1014 => f("(unknown)", U16),
        1015 => f("enabled", U8),
        1016 => f("gate adjust", S32),
        1017 => f("transpose", S32),
        1018 => f("colour (255 = none)", U8),
        1019 => f("tick adjust measure", U32),
        1020 => f("(unknown)", Raw),
        1021 => f("range low", U8),
        1022 => f("range high", U8),
        2009 => f("end of track", Container),
        _ => None,
    }
}

/// Event container names (IDs 2001 and up inside a track).
pub fn event_name(id: u16) -> Option<&'static str> {
    Some(match id {
        2001 => "note",
        2002 => "program change",
        2003 => "control (CCM)",
        2004 => "sysex",
        2005 => "comment / text",
        2007 => "variables",
        2008 => "tempo",
        2009 => "end of track",
        2011 => "lyric",
        2012 => "cue point",
        2014 => "measure link",
        2015 => "time signature",
        2016 => "key signature",
        2017 => "marker",
        2018 => "scale",
        2019 => "chord",
        _ => return None,
    })
}

/// Fields inside an event container. Every documented event starts with
/// `1001: u32 position`.
pub fn event_field(event: u16, id: u16) -> Option<Field> {
    if id == 1001 {
        return f("position", U32);
    }
    match (event, id) {
        (2001, 2001) => f("key", U8),
        (2001, 2002) => f("velocity", U8),
        (2001, 2003) => f("gate", U32),
        (2002, 2001) => f("bank MSB? (-1 = none)", S8),
        (2002, 2002) => f("bank LSB? (-1 = none)", S8),
        (2002, 2003) => f("patch (program + 1)", U8),
        (2002, 2004) => f("(unknown)", Raw),
        (2002, 2005) => f("(unknown)", U8),
        (2002, 2006) => f("(unknown)", S16),
        (2003, 2001) => f("CCM id", U16),
        (2003, 2002) => f("gate", F32List),
        (2003, 2003) => f("value", F32List),
        (2004, 2001) => f("text", String),
        (2004, 2002) => f("sysex", Raw),
        (2005 | 2011 | 2012 | 2017, 2001) => f("text", String),
        (2007, 2001) => f("variable", String),
        (2007, 2002) => f("value", String),
        (2008, 2001) => f("BPM", F32List),
        (2014, 2001) => f("destination measure", U32),
        (2014, 2002) => f("key transpose", S32),
        (2015, 2001) => f("numerator", U8),
        (2015, 2002) => f("denominator", U8),
        (2016, 2001) => f("key", U8),
        (2018 | 2019, 2001) => f("root key", U8),
        (2018 | 2019, 2002) => f("type id", U32),
        (2018 | 2019, 2004) => f("name", String),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_scoped_by_parent() {
        assert_eq!(root_field(1001).unwrap().name, "copyright");
        assert_eq!(track_field(1001).unwrap().name, "channel (0-based)");
        assert_eq!(event_field(2001, 1001).unwrap().name, "position");
        assert_eq!(event_field(2003, 2003).unwrap().kind, FieldKind::F32List);
        assert_eq!(event_name(2008), Some("tempo"));
        assert!(event_field(2001, 9999).is_none());
    }
}
