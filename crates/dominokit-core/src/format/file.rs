//! Moving clips between the clipboard payload, `.dms` project files and
//! `.mid` files.
//!
//! A clipboard payload and a `.dms` file share one envelope and chunk grammar
//! (`PortalSequenceData` + size + zlib). What differs is the content: a
//! project starts with a conductor track (track `1003 = 1`) and each track's
//! `2009` is its own end of track, whereas a clipboard copy has no conductor
//! and every track's `2009` is the copied range's length.

use std::io::Write;
use std::path::Path;

use thiserror::Error;

use crate::format::chunk;
use crate::format::clip::{check_format_limits, decode_clip, verify_encoding, Clip, EncodeOptions, StreamPlan, TopNode, Track, TrackChild};
use crate::format::envelope::{self, EnvelopeError, Magic};
use crate::format::midi;
use crate::format::smf::SmfError;
use crate::format::template::{TRACK_HEAD, TRACK_TAIL};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    /// Domino project (`PortalSequenceData`), also accepts `.dmp` part files.
    Dms,
    /// Standard MIDI File.
    Mid,
}

impl FileKind {
    pub fn from_path(path: &Path) -> Option<FileKind> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        match ext.as_str() {
            "dms" | "dmp" => Some(FileKind::Dms),
            "mid" | "midi" | "smf" => Some(FileKind::Mid),
            _ => None,
        }
    }

    pub const fn extension(self) -> &'static str {
        match self {
            FileKind::Dms => "dms",
            FileKind::Mid => "mid",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            FileKind::Dms => "Domino project (.dms)",
            FileKind::Mid => "MIDI file (.mid)",
        }
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum LoadError {
    #[error("not a Domino (.dms) or MIDI (.mid) file")]
    UnknownFormat,
    #[error("{0}")]
    Envelope(String),
    #[error("{0}")]
    Midi(#[from] SmfError),
}

impl From<EnvelopeError> for LoadError {
    fn from(e: EnvelopeError) -> Self {
        LoadError::Envelope(describe_envelope_error(&e))
    }
}

pub fn describe_envelope_error(e: &EnvelopeError) -> String {
    match e {
        EnvelopeError::BadMagic => "not Domino data (wrong header)".into(),
        EnvelopeError::Zlib(z) => format!("damaged Domino data (zlib: {z})"),
        EnvelopeError::SizeMismatch { declared, actual } => {
            format!("damaged Domino data: header says {declared} bytes, decompressed {actual}")
        }
    }
}

/// A decoded clip and the kind of file it came from.
#[derive(Debug, Clone)]
pub struct Loaded {
    pub clip: Clip,
    pub kind: FileKind,
}

/// Decode a clipboard payload or `.dms`/`.dmp` file body.
pub fn decode_payload(bytes: &[u8]) -> Result<Loaded, EnvelopeError> {
    let magic = if bytes.starts_with(Magic::PortalPartData.as_bytes()) {
        Magic::PortalPartData
    } else {
        Magic::PortalSequenceData
    };
    let env = envelope::decode(bytes, magic)?;
    Ok(Loaded { clip: decode_clip(&env.data), kind: FileKind::Dms })
}

/// Load a file's bytes, detecting the format from the content.
pub fn load(bytes: &[u8]) -> Result<Loaded, LoadError> {
    if bytes.starts_with(b"MThd") {
        return Ok(Loaded { clip: midi::clip_from_smf_bytes(bytes)?, kind: FileKind::Mid });
    }
    if bytes.starts_with(b"Portal") {
        return Ok(decode_payload(bytes)?);
    }
    Err(LoadError::UnknownFormat)
}

/// Encode a clip as file bytes. Returns warnings about anything a `.mid`
/// cannot represent, or an error when the clip exceeds what the format can
/// store.
pub fn save(clip: &Clip, kind: FileKind) -> Result<(Vec<u8>, Vec<String>), String> {
    match kind {
        FileKind::Dms => Ok((to_dms(clip)?, Vec::new())),
        FileKind::Mid => midi::smf_bytes_from_clip(clip),
    }
}

/// Encode a clip straight into a file, streaming: memory use stays small
/// whatever the clip size. The file is written under a temporary name and
/// renamed when complete, so a failure never leaves half a file behind.
pub fn save_to(clip: &Clip, kind: FileKind, path: &Path) -> Result<Vec<String>, String> {
    let tmp = path.with_extension(format!("{}.part", kind.extension()));
    let result = (|| {
        let file = std::fs::File::create(&tmp).map_err(|e| format!("could not create the file: {e}"))?;
        let mut w = std::io::BufWriter::with_capacity(4 << 20, file);
        let warnings = match kind {
            FileKind::Dms => {
                write_dms(clip, &mut w)?;
                Vec::new()
            }
            FileKind::Mid => midi::write_smf(clip, &mut w)?,
        };
        w.flush().map_err(|e| format!("could not write the file: {e}"))?;
        Ok(warnings)
    })();
    match result {
        Ok(warnings) => {
            std::fs::rename(&tmp, path).map_err(|e| format!("could not finish the file: {e}"))?;
            Ok(warnings)
        }
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

/// Encoded streams up to this size are fully re-checked before writing
/// (encode → count the notes in the bytes). Bigger ones are produced
/// block by block and never exist whole, so they rely on the stream
/// encoder, which is tested against the full encoder byte for byte.
const VERIFY_BYTES: usize = 64 << 20;

/// Encode, check and compress a clip into `sink`, a block at a time.
fn write_envelope(
    clip: &Clip,
    opts: &EncodeOptions,
    notes: usize,
    sink: impl FnMut(&[u8]) -> std::io::Result<()>,
) -> Result<(), String> {
    check_format_limits(clip, opts)?;
    let plan = StreamPlan::new(clip, opts);
    let io = |e: std::io::Error| format!("could not write: {e}");
    if plan.len() <= VERIFY_BYTES {
        let mut unpacked = vec![0u8; plan.len()];
        plan.fill(0, &mut unpacked);
        verify_encoding(notes, &unpacked)?;
        return envelope::encode_streamed(unpacked.len(), Magic::PortalSequenceData, |at, buf| buf.copy_from_slice(&unpacked[at..at + buf.len()]), sink).map_err(io);
    }
    envelope::encode_streamed(plan.len(), Magic::PortalSequenceData, |at, buf| plan.fill(at, buf), sink).map_err(io)
}

fn dms_options(clip: &Clip) -> EncodeOptions {
    EncodeOptions {
        insert_before_first_track: (!clip.tracks().any(|t| t.is_conductor())).then(|| conductor_track(clip.span())),
        ..Default::default()
    }
}

/// Stream a clip as a `.dms` file into `w`.
pub fn write_dms(clip: &Clip, w: &mut impl Write) -> Result<(), String> {
    write_envelope(clip, &dms_options(clip), clip.note_count(), |b| w.write_all(b))
}

/// `.dms` bytes. A clip without a conductor track (a clipboard copy) gets an
/// empty one in front so Domino opens every copied track as a normal track.
pub fn to_dms(clip: &Clip) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    write_dms(clip, &mut out)?;
    Ok(out)
}

/// An empty conductor track, built from Domino's own track template.
pub fn conductor_track(length: u32) -> Track {
    let mut conductor = Track::default();
    conductor.children.extend(chunk::read_all(TRACK_HEAD).items.into_iter().map(TrackChild::raw));
    conductor.set_setting(1003, vec![1]);
    conductor.children.push(TrackChild::length(length));
    conductor.children.extend(chunk::read_all(TRACK_TAIL).items.into_iter().map(TrackChild::raw));
    conductor
}

/// The clip with an empty conductor track inserted before its first track,
/// unless it already has one.
pub fn with_conductor(clip: &Clip) -> Clip {
    let mut out = clip.clone();
    if !clip.tracks().any(|t| t.is_conductor()) {
        let at = out.nodes.iter().position(|n| matches!(n, TopNode::Track(_))).unwrap_or(out.nodes.len());
        out.nodes.insert(at, TopNode::Track(conductor_track(clip.span())));
    }
    out
}

/// Clipboard payload for a clip: optionally without the conductor track
/// (whose events would otherwise land in the highlighted track), and with
/// every track given the same `2009` length, as a real Domino copy has.
/// The clip itself is not copied or changed.
pub fn clipboard_payload(clip: &Clip, skip_conductor: bool) -> Result<Vec<u8>, String> {
    let kept = |t: &&Track| !(skip_conductor && t.is_conductor());
    if clip.tracks().filter(kept).count() == 0 {
        return Err("nothing to write: the clip has no tracks".into());
    }
    let mut lengths = clip.tracks().filter(kept).map(|t| t.length());
    let first = lengths.next().flatten();
    let uniform = first.is_some() && lengths.all(|l| l == first);
    let notes: usize = clip.tracks().filter(kept).map(Track::note_count).sum();
    let opts = EncodeOptions {
        skip_conductor,
        uniform_length: (!uniform).then(|| clip.tracks().filter(kept).map(Track::span).max().unwrap_or(1)),
        ..Default::default()
    };
    // only the compressed payload is ever held (the clipboard needs it whole)
    let mut out = Vec::new();
    write_envelope(clip, &opts, notes, |b| {
        out.extend_from_slice(b);
        Ok(())
    })?;
    out.shrink_to_fit();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::clip::encode_clip;
    use crate::format::template::tests::golden_unpacked;

    fn golden() -> Clip {
        decode_clip(&golden_unpacked())
    }

    fn unpack(payload: &[u8]) -> Vec<u8> {
        envelope::decode(payload, Magic::PortalSequenceData).unwrap().data
    }

    #[test]
    fn clipboard_identity_is_byte_exact() {
        assert_eq!(unpack(&clipboard_payload(&golden(), true).unwrap()), golden_unpacked());
    }

    #[test]
    fn dms_gets_a_conductor_and_comes_back() {
        let loaded = load(&to_dms(&golden()).unwrap()).unwrap();
        assert_eq!(loaded.kind, FileKind::Dms);
        assert_eq!(loaded.clip.track_count(), 2);
        assert!(loaded.clip.tracks().next().unwrap().is_conductor());
        // writing it back to the clipboard drops the conductor again
        assert_eq!(unpack(&clipboard_payload(&loaded.clip, true).unwrap()), golden_unpacked());
        // a project keeps its own conductor
        assert_eq!(load(&to_dms(&loaded.clip).unwrap()).unwrap().clip.track_count(), 2);
        assert_eq!(encode_clip(&with_conductor(&golden())), encode_clip(&loaded.clip));
    }

    #[test]
    fn project_track_lengths_are_unified_for_the_clipboard() {
        let mut clip = golden();
        let mut second = clip.tracks().next().unwrap().clone();
        second.set_length(4000);
        clip.nodes.push(TopNode::Track(second));
        let out = decode_clip(&unpack(&clipboard_payload(&clip, true).unwrap()));
        assert!(out.tracks().all(|t| t.length() == Some(4000)));
    }

    #[test]
    fn mid_round_trip_keeps_notes() {
        let (bytes, warnings) = save(&golden(), FileKind::Mid).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        let loaded = load(&bytes).unwrap();
        assert_eq!(loaded.kind, FileKind::Mid);
        let notes: Vec<_> = loaded.clip.note_views().iter().map(|n| (n.tick, n.gate, n.key, n.vel)).collect();
        assert_eq!(notes, vec![(0, 240, 36, 100), (240, 240, 38, 90), (480, 480, 127, 64)]);
    }

    #[test]
    fn saving_to_a_file_streams_the_same_bytes() {
        let dir = std::env::temp_dir().join(format!("dominokit-save-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for kind in [FileKind::Dms, FileKind::Mid] {
            let path = dir.join(format!("x.{}", kind.extension()));
            save_to(&golden(), kind, &path).unwrap();
            assert_eq!(std::fs::read(&path).unwrap(), save(&golden(), kind).unwrap().0);
            assert!(!path.with_extension(format!("{}.part", kind.extension())).exists());
        }
    }

    #[test]
    fn unknown_bytes_are_rejected() {
        assert_eq!(load(b"hello").unwrap_err(), LoadError::UnknownFormat);
    }
}
