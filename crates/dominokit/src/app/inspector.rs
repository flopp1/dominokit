//! Inspector: the working copy's chunks, named by the schema (root / track /
//! per-event fields), with decoded values and hex.
//!
//! It shows one section at a time (the song's own chunks, or one track),
//! one line per chunk, and lays out only the rows on screen; long tracks
//! are split into pages so even millions of notes stay instant.

use eframe::egui::{self, RichText};

use super::jobs::Action;
use super::state::{count, App};
use super::theme::WARN;
use dominokit_core::format::chunk::{self, Item};
use dominokit_core::format::clip::{Clip, TopNode, Track, TrackChild};
use dominokit_core::format::events::field;
use dominokit_core::format::schema::{event_field, event_name, root_field, track_field, Field, FieldKind};

const PAGE: usize = 10_000;

#[derive(Default)]
pub struct Inspector {
    /// 0 = song chunks, k = track k.
    section: usize,
    page: usize,
    generation: u64,
}

pub fn show(view: &mut Inspector, app: &mut App, ui: &mut egui::Ui) {
    if view.generation != app.load_id {
        *view = Inspector { generation: app.load_id, ..Default::default() };
    }
    let Some(clip) = app.source.as_ref() else {
        ui.weak(if app.stats.is_some() { "Working…" } else { "Nothing loaded." });
        return;
    };
    let tracks: Vec<&Track> = clip.tracks().collect();
    view.section = view.section.min(tracks.len());

    let mut save = false;
    ui.horizontal(|ui| {
        let label = |k: usize| -> String {
            if k == 0 {
                "Song chunks".into()
            } else {
                let t = tracks[k - 1];
                let name = t.display_name().filter(|n| !n.is_empty()).map(|n| format!(" “{n}”")).unwrap_or_default();
                format!("Track {k}{name}{}", if t.is_conductor() { " (conductor)" } else { "" })
            }
        };
        let before = view.section;
        egui::ComboBox::from_id_source("inspector_section").width(260.0).selected_text(label(view.section)).show_ui(ui, |ui| {
            for k in 0..=tracks.len() {
                ui.selectable_value(&mut view.section, k, label(k));
            }
        });
        if view.section != before {
            view.page = 0;
        }
        save = ui.button("Save dump to file…").on_hover_text("Write the working copy's unpacked chunk bytes").clicked();
        ui.weak(format!("{} warning(s)", clip.warnings.len()));
    });
    for w in &clip.warnings {
        ui.colored_label(WARN, format!("⚠ {w}"));
    }

    let rows = if view.section == 0 { root_rows(clip) } else { tracks[view.section - 1].children.len() };
    let pages = rows.div_ceil(PAGE).max(1);
    view.page = view.page.min(pages - 1);
    if pages > 1 {
        ui.horizontal(|ui| {
            if ui.add_enabled(view.page > 0, egui::Button::new("◀")).clicked() {
                view.page -= 1;
            }
            let mut shown = view.page + 1;
            ui.add(egui::DragValue::new(&mut shown).clamp_range(1..=pages).prefix("page "));
            view.page = shown - 1;
            ui.weak(format!("of {} ({} chunks)", count(pages), count(rows)));
            if ui.add_enabled(view.page + 1 < pages, egui::Button::new("▶")).clicked() {
                view.page += 1;
            }
        });
    }
    ui.separator();

    let first = view.page * PAGE;
    let on_page = (rows - first).min(PAGE);
    let row_h = ui.text_style_height(&egui::TextStyle::Monospace) + 4.0;
    egui::ScrollArea::both().auto_shrink([false, false]).id_source(("inspector", view.section, view.page)).show_rows(ui, row_h, on_page, |ui, range| {
        for r in range {
            let i = first + r;
            if view.section == 0 {
                root_row(ui, clip, i);
            } else {
                child_row(ui, &tracks[view.section - 1].children[i], i);
            }
        }
    });

    if save {
        if let Some(path) = rfd::FileDialog::new().set_file_name("dominokit-dump.bin").save_file() {
            app.request(Action::Dump(path));
        }
    }
}

fn root_rows(clip: &Clip) -> usize {
    clip.nodes.len() + (!clip.trailing.is_empty()) as usize
}

fn root_row(ui: &mut egui::Ui, clip: &Clip, i: usize) {
    match clip.nodes.get(i) {
        Some(TopNode::Raw(item)) => line(ui, i, item.tag, field_label(root_field(item.tag)), item.body.len(), value(item, root_field(item.tag))),
        Some(TopNode::Track(t)) => {
            let name = t.display_name().filter(|n| !n.is_empty()).map(|n| format!(" “{n}”")).unwrap_or_default();
            line(ui, i, 1003, format!("track{name}"), 0, format!("{} chunks — pick it above", count(t.children.len())));
        }
        None => {
            ui.colored_label(WARN, format!("trailing {} byte(s): {}", clip.trailing.len(), hex(&clip.trailing)));
        }
    }
}

fn child_row(ui: &mut egui::Ui, child: &TrackChild, i: usize) {
    match child {
        TrackChild::Raw(item) => line(ui, i, item.tag, field_label(track_field(item.tag)), item.body.len(), value(item, track_field(item.tag))),
        TrackChild::Note(n) => line(ui, i, 2001, "note".into(), 34, format!("@{}  key {}  vel {}  gate {}", n.tick, n.key, n.vel, n.gate)),
        TrackChild::OddNote(_) => {
            let fields = child.note_fields();
            line(ui, i, 2001, "note (unusual layout)".into(), child.encoded_len() - 6, fields_text(2001, &fields));
        }
        TrackChild::Length(l) => line(ui, i, 2009, "end of track".into(), l.encode().len() - 6, format!("@{}", l.length)),
        TrackChild::Event(e) => {
            let name = event_name(e.id).map(str::to_string).unwrap_or_else(|| format!("event {}", e.id));
            let parsed = chunk::read_all(&e.item.body);
            let text = if parsed.is_clean() { fields_text(e.id, &parsed.items) } else { hex(&e.item.body) };
            line(ui, i, e.id, name, e.item.body.len(), text);
        }
    }
}

/// "position 120 · CCM id 7 · value f32 [100]"
fn fields_text(event: u16, fields: &[Item]) -> String {
    fields
        .iter()
        .map(|f| {
            let schema = event_field(event, f.tag);
            format!("{} {}", schema.map_or_else(|| f.tag.to_string(), |s| s.name.to_string()), value(f, schema))
        })
        .collect::<Vec<_>>()
        .join(" · ")
}

fn line(ui: &mut egui::Ui, i: usize, tag: u16, name: String, size: usize, value: String) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(format!("{i:>8}")).monospace().weak());
        ui.label(RichText::new(format!("{tag:>5}")).monospace());
        ui.add_sized([180.0, 16.0], egui::Label::new(name).truncate(true));
        ui.add_sized([56.0, 16.0], egui::Label::new(RichText::new(format!("{size} B")).weak()));
        ui.label(RichText::new(value).monospace());
    });
}

fn field_label(field: Option<Field>) -> String {
    field.map_or("unknown".into(), |f| f.name.to_string())
}

fn value(item: &Item, field: Option<Field>) -> String {
    let b = &item.body;
    match field.map(|f| f.kind) {
        Some(FieldKind::F32List) => match field_f32s(b) {
            Some(v) => format!("f32 [{v}]"),
            None => hex(b),
        },
        Some(FieldKind::String) => format!("{:?}", String::from_utf8_lossy(b)),
        Some(FieldKind::S8) if b.len() == 1 => (b[0] as i8).to_string(),
        Some(FieldKind::S16) if b.len() == 2 => i16::from_le_bytes([b[0], b[1]]).to_string(),
        Some(FieldKind::S32) if b.len() == 4 => i32::from_le_bytes([b[0], b[1], b[2], b[3]]).to_string(),
        _ => match b.len() {
            0 => "(empty)".into(),
            1 => b[0].to_string(),
            2 => u16::from_le_bytes([b[0], b[1]]).to_string(),
            4 => u32::from_le_bytes([b[0], b[1], b[2], b[3]]).to_string(),
            _ => hex(b),
        },
    }
}

fn field_f32s(body: &[u8]) -> Option<String> {
    let parsed = chunk::read_all(body);
    if !parsed.is_clean() || parsed.items.iter().any(|c| c.tag != 0 || c.body.len() != 4) {
        return field(body, 0).filter(|v| v.len() == 4).map(|v| f32::from_le_bytes(v.try_into().unwrap()).to_string());
    }
    Some(parsed.items.iter().map(|c| f32::from_le_bytes(c.body[..4].try_into().unwrap()).to_string()).collect::<Vec<_>>().join(", "))
}

fn hex(bytes: &[u8]) -> String {
    let shown: Vec<String> = bytes.iter().take(24).map(|b| format!("{b:02X}")).collect();
    format!("{}{}", shown.join(" "), if bytes.len() > 24 { " …" } else { "" })
}
