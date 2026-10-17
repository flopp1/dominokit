//! Checks against real files made by Domino (see `probe/FINDINGS.md`):
//! `probe.mid` was imported into Domino and saved as `probe.dms`, which Domino
//! then exported as `probe_out.mid`. `clip1.bin` is a real clipboard copy.

use std::path::PathBuf;

use dominokit_core::format::clip::{encode_clip, Clip, TrackChild};
use dominokit_core::format::envelope::{self, Magic};
use dominokit_core::format::file::{self, FileKind};
use dominokit_core::format::smf::{self, SmfEvent};

fn probe(name: &str) -> Vec<u8> {
    let path: PathBuf = [env!("CARGO_MANIFEST_DIR"), "..", "..", "probe", name].iter().collect();
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// Per track: every child as (tag, encoded bytes).
fn children(clip: &Clip) -> Vec<Vec<(u16, Vec<u8>)>> {
    clip.tracks()
        .map(|t| {
            t.children
                .iter()
                .map(|c| match c {
                    TrackChild::Raw(i) => (i.tag, i.encode()),
                    TrackChild::Event(e) => (e.id, e.encode()),
                    other => (if other.is_note() { 2001 } else { 2009 }, other.encode()),
                })
                .collect()
        })
        .collect()
}

#[test]
fn real_files_round_trip_byte_exact() {
    for name in ["probe.dms", "probe2.dms", "clip1.bin"] {
        let bytes = probe(name);
        let loaded = file::load(&bytes).unwrap();
        assert!(loaded.clip.warnings.is_empty(), "{name}: {:?}", loaded.clip.warnings);
        let unpacked = envelope::decode(&bytes, Magic::PortalSequenceData).unwrap().data;
        assert_eq!(encode_clip(&loaded.clip), unpacked, "{name}");
        // notes in Domino's own files all use the compact canonical form
        assert!(
            loaded.clip.tracks().all(|t| !t.children.iter().any(|c| matches!(c, TrackChild::OddNote(_)))),
            "{name}"
        );
    }
    let dms = file::load(&probe("probe.dms")).unwrap().clip;
    assert_eq!(dms.track_count(), 9);
    assert!(dms.tracks().next().unwrap().is_conductor());
    assert!(!dms.tracks().nth(1).unwrap().is_conductor());
}

#[test]
fn midi_import_matches_dominos_own_import() {
    let mine = file::load(&probe("probe.mid")).unwrap().clip;
    let domino = file::load(&probe("probe.dms")).unwrap().clip;
    assert_eq!(mine.ppq(), domino.ppq());
    let (a, b) = (children(&mine), children(&domino));
    assert_eq!(a.len(), b.len());
    for (t, (mine_t, domino_t)) in a.iter().zip(&b).enumerate() {
        // track 1005 is undocumented editor state that Domino varies per track
        let strip = |v: &Vec<(u16, Vec<u8>)>| -> Vec<(u16, Vec<u8>)> {
            v.iter().filter(|(tag, _)| *tag != 1005).cloned().collect()
        };
        let (mine_t, domino_t) = (strip(mine_t), strip(domino_t));
        for (k, (x, y)) in mine_t.iter().zip(&domino_t).enumerate() {
            assert_eq!(x, y, "track {t}, child {k}");
        }
        assert_eq!(mine_t.len(), domino_t.len(), "track {t} child count");
    }
}

/// Every track as (tick, event). Key signatures are left out: Domino's export
/// moves them to the start of their measure and keeps only the last one per
/// measure, while DominoKit writes them at their real ticks.
fn smf_events(bytes: &[u8]) -> Vec<Vec<(u32, SmfEvent)>> {
    let is_key_sig = |e: &SmfEvent| matches!(e, SmfEvent::Meta { kind: 0x59, .. });
    smf::parse(bytes)
        .unwrap()
        .tracks
        .into_iter()
        .map(|t| t.events.into_iter().filter(|(_, e)| !is_key_sig(e)).collect())
        .collect()
}

#[test]
fn midi_export_matches_dominos_own_export() {
    for (dms, out) in [("probe.dms", "probe_out.mid"), ("probe2.dms", "probe2_out.mid")] {
        let clip = file::load(&probe(dms)).unwrap().clip;
        let (mine, _warnings) = file::save(&clip, FileKind::Mid).unwrap();
        let (a, b) = (smf_events(&mine), smf_events(&probe(out)));
        assert_eq!(a.len(), b.len(), "{dms}");
        for (t, (x, y)) in a.iter().zip(&b).enumerate() {
            for (k, (e1, e2)) in x.iter().zip(y).enumerate() {
                assert_eq!(e1, e2, "{dms}: track {t}, event {k}");
            }
            assert_eq!(x.len(), y.len(), "{dms}: track {t} event count");
        }
    }
}
