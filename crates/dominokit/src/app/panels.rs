//! Header with the clipboard / file toolbar, status bar, and the side panel
//! with the two tool cards: Flip and Chop.

use std::path::PathBuf;

use eframe::egui::{self, Button, RichText};

use dominokit_core::format::file::FileKind;

use super::state::{count, App};
use super::theme::{ACCENT, ACCENT_TEXT, ERROR, WARN};
use super::jobs::Action;

// ---- header and status --------------------------------------------------------

pub fn header(app: &mut App, ctx: &egui::Context, busy: bool) {
    egui::TopBottomPanel::top("header").show(ctx, |ui| {
        ui.add_enabled_ui(!busy, |ui| header_body(app, ui));
    });
}

fn header_body(app: &mut App, ui: &mut egui::Ui) {
    {
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("DominoKit").size(20.0).strong());
            ui.separator();
            match &app.stats {
                Some(stats) => {
                    let ppq = stats.ppq.map(|p| p.to_string()).unwrap_or_else(|| "?".into());
                    ui.label(RichText::new(stats.summary()).strong());
                    ui.label(format!("· PPQ {ppq} · {} ticks", count(stats.span as usize)));
                    if let Some(origin) = &app.origin {
                        ui.weak(format!("· {}", origin.label()));
                    }
                }
                None => {
                    ui.weak("Working copy is empty");
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_enabled(app.source.is_some(), Button::new("Clear"))
                    .on_hover_text("Empty the working copy. The clipboard and files are not touched.")
                    .clicked()
                {
                    app.request(Action::Clear);
                }
                let steps = app.undo_len();
                let undo = if steps == 0 { "Undo".to_string() } else { format!("Undo ({steps})") };
                if ui
                    .add_enabled(steps > 0, Button::new(undo))
                    .on_hover_text("Undo the last Flip or Chop (Ctrl+Z)")
                    .clicked()
                {
                    app.request(Action::Undo);
                }
            });
        });
        ui.add_space(2.0);
        toolbar(app, ui);
        if let Some(stats) = &app.stats {
            if !stats.tracks.is_empty() {
                ui.label(
                    RichText::new("Domino does not convert PPQ when pasting — make sure the destination song uses the same PPQ.")
                        .small()
                        .color(WARN),
                );
            }
        }
        ui.add_space(4.0);
    }
}

pub fn status_bar(app: &App, ctx: &egui::Context) {
    egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
        ui.add_space(3.0);
        ui.horizontal_wrapped(|ui| {
            if app.status_is_error {
                ui.label(RichText::new(format!("⚠ {}", app.status)).color(ERROR));
            } else {
                ui.label(&app.status);
            }
        });
        ui.add_space(3.0);
    });
}

// ---- side panel -------------------------------------------------------------

pub fn side_panel(app: &mut App, ctx: &egui::Context, busy: bool) {
    egui::SidePanel::left("tools")
        .default_width(330.0)
        .min_width(300.0)
        .resizable(true)
        .show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.add_space(6.0);
                ui.add_enabled_ui(!busy, |ui| flip_card(app, ui));
                ui.add_space(6.0);
                ui.add_enabled_ui(!busy, |ui| chop_card(app, ui));
                ui.add_space(6.0);
            });
        });
}

fn card(ui: &mut egui::Ui, title: &str, subtitle: &str, body: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::group(ui.style())
        .inner_margin(egui::Margin::same(10.0))
        .rounding(8.0)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(title).size(16.0).strong());
            ui.weak(subtitle);
            ui.add_space(4.0);
            body(ui);
        });
}

fn primary(ui: &mut egui::Ui, enabled: bool, text: &str) -> egui::Response {
    ui.add_enabled(
        enabled,
        Button::new(RichText::new(text).strong().color(ACCENT_TEXT))
            .fill(ACCENT)
            .min_size(egui::vec2(ui.available_width(), 30.0)),
    )
}

/// Clipboard and file transfers, as a toolbar under the header.
fn toolbar(app: &mut App, ui: &mut egui::Ui) {
    let loaded = app.source.is_some();
    let tall = egui::vec2(0.0, 28.0);
    ui.horizontal(|ui| {
        ui.weak("Clipboard");
        if ui
            .add(Button::new("⬇  Read").min_size(tall))
            .on_hover_text("Load what you copied in Domino into the working copy (Ctrl+R).\nReplaces the current working copy and its undo history.")
            .clicked()
        {
            app.request(Action::Read);
        }
        if ui
            .add_enabled(loaded, Button::new("⬆  Write").min_size(tall))
            .on_hover_text("Put the working copy on the clipboard, replacing its contents (Ctrl+W).\nThen paste with Ctrl+V in Domino.")
            .clicked()
        {
            app.request(Action::Write);
        }
        ui.separator();
        ui.weak("Files");
        if ui
            .add(Button::new("📂  Open…").min_size(tall))
            .on_hover_text("Load a .dms or .mid file into the working copy (Ctrl+O)")
            .clicked()
        {
            open_file_dialog(app);
        }
        ui.add_enabled_ui(loaded, |ui| {
            ui.menu_button("💾  Save as  ⏷", |ui| {
                ui.weak("Save the working copy as…");
                for kind in [FileKind::Dms, FileKind::Mid] {
                    if ui.button(kind.label()).clicked() {
                        ui.close_menu();
                        if let Some(path) = save_dialog(app, kind, "dominokit") {
                            app.request(Action::Save(path, kind));
                        }
                    }
                }
            })
            .response
            .on_hover_text("Save the working copy as a .dms or .mid file");
        });
        ui.separator();
        ui.menu_button("Clipboard ↔ file  ⏷", |ui| {
            ui.weak("Direct transfers; the working copy is not touched.");
            for kind in [FileKind::Dms, FileKind::Mid] {
                if ui.button(format!("Save the clipboard as {}…", kind.label())).clicked() {
                    ui.close_menu();
                    if let Some(path) = save_dialog(app, kind, "clipboard") {
                        app.request(Action::ClipboardToFile(path, kind));
                    }
                }
            }
            if ui.button("Put a .dms / .mid file on the clipboard…").clicked() {
                ui.close_menu();
                if let Some(path) = pick_dialog(app) {
                    app.request(Action::FileToClipboard(path));
                }
            }
        })
        .response
        .on_hover_text("Move data between the clipboard and a file without loading it");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.checkbox(&mut app.settings.skip_conductor, "Skip conductor track when writing").on_hover_text(
                "Domino projects (.dms / .mid) start with a conductor track (tempo, signatures).\n\
                 Pasting it would put those events into the highlighted track.",
            );
        });
    });
}

/// "C4 · MIDI 60" for a 1-based UI key.
pub fn key_label(ui_key: u16) -> String {
    const NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
    let byte = ui_key.clamp(1, 256) - 1;
    if byte <= 127 {
        format!("{}{} · MIDI {byte}", NAMES[byte as usize % 12], byte as i32 / 12 - 1)
    } else {
        format!("beyond MIDI ({byte})")
    }
}

fn flip_card(app: &mut App, ui: &mut egui::Ui) {
    card(
        ui,
        "Flip",
        "Mirror the notes between two keys, in every track.",
        |ui| {
            egui::Grid::new("flip_grid").num_columns(3).spacing([8.0, 6.0]).show(ui, |ui| {
                ui.label("From key");
                ui.add(egui::DragValue::new(&mut app.settings.flip_from).clamp_range(1..=256).speed(0.25));
                ui.weak(key_label(app.settings.flip_from));
                ui.end_row();
                ui.label("To key");
                ui.add(egui::DragValue::new(&mut app.settings.flip_to).clamp_range(1..=256).speed(0.25));
                ui.weak(key_label(app.settings.flip_to));
                ui.end_row();
            });
            ui.horizontal_wrapped(|ui| {
                ui.weak("Presets:");
                for (label, a, b, tip) in [
                    ("Full MIDI", 1u16, 128u16, "keys 1–128 (MIDI 0–127): k → 127 − k"),
                    ("All keys", 1, 256, "keys 1–256"),
                    ("Clip's range", 0, 0, "lowest to highest note in the working copy"),
                ] {
                    if ui.small_button(label).on_hover_text(tip).clicked() {
                        let range = if a == 0 { clip_key_range(app) } else { Some((a, b)) };
                        if let Some((a, b)) = range {
                            app.settings.flip_from = a;
                            app.settings.flip_to = b;
                        }
                    }
                }
            });
            let r = app.flip_range();
            let (lo, hi) = (r.lo as u16 + 1, r.hi as u16 + 1);
            ui.weak(if hi > lo + 1 {
                format!("Key {lo} ↔ {hi}, {} ↔ {}, … Notes outside stay put.", lo + 1, hi - 1)
            } else {
                format!("Key {lo} ↔ {hi}. Notes outside stay put.")
            });
            ui.add_space(2.0);
            if primary(ui, app.source.is_some(), "Flip").clicked() {
                app.request(Action::Flip);
            }
        },
    );
}

/// Lowest/highest note of the working copy, as 1-based UI keys.
fn clip_key_range(app: &App) -> Option<(u16, u16)> {
    let (lo, hi) = app.stats.as_ref()?.keys?;
    Some((lo as u16 + 1, hi as u16 + 1))
}

fn chop_card(app: &mut App, ui: &mut egui::Ui) {
    card(
        ui,
        "Chop",
        "Slice every note at a fixed grid from the start of the clip, in every track.",
        |ui| {
            ui.horizontal(|ui| {
                ui.label("Every");
                ui.add(
                    egui::TextEdit::singleline(&mut app.settings.chop_text)
                        .desired_width(90.0)
                        .hint_text("ticks"),
                )
                .on_hover_text("Ticks between cuts. Whole ticks (120) or a fraction (100/3).");
                ui.label("ticks");
            });
            match app.chop_step() {
                Ok(step) if !step.is_whole() => {
                    ui.weak(format!("≈ {:.3} ticks — each cut is rounded to the nearest tick, without drift.", step.as_f64()));
                }
                Ok(_) => {}
                Err(e) => {
                    ui.colored_label(ERROR, e);
                }
            }

            ui.add_space(2.0);
            ui.weak("…or fill it from a note length:");
            let before = (
                app.settings.chop_ppq,
                app.settings.chop_denominator,
                app.settings.chop_modifier.clone(),
            );
            ui.horizontal(|ui| {
                ui.label("PPQ");
                ui.add(egui::DragValue::new(&mut app.settings.chop_ppq).clamp_range(1..=30_000))
                    .on_hover_text("Ticks per quarter note. Set from the clip when one is loaded.");
                egui::ComboBox::from_id_source("chop_den")
                    .width(64.0)
                    .selected_text(format!("1/{}", app.settings.chop_denominator))
                    .show_ui(ui, |ui| {
                        for d in [1u32, 2, 4, 8, 16, 32, 64] {
                            ui.selectable_value(&mut app.settings.chop_denominator, d, format!("1/{d}"));
                        }
                    });
                egui::ComboBox::from_id_source("chop_mod")
                    .width(72.0)
                    .selected_text(app.settings.chop_modifier.clone())
                    .show_ui(ui, |ui| {
                        for m in ["plain", "dotted", "triplet"] {
                            ui.selectable_value(&mut app.settings.chop_modifier, m.to_string(), m);
                        }
                    });
            });
            let after = (
                app.settings.chop_ppq,
                app.settings.chop_denominator,
                app.settings.chop_modifier.clone(),
            );
            if before != after {
                // the computed length goes straight into the box above
                app.fill_chop_from_note_value();
            }
            match app.note_value_step() {
                Ok(step) if app.chop_matches_note_value() => {
                    ui.weak(format!(
                        "1/{} {} at PPQ {} = {} ticks",
                        app.settings.chop_denominator,
                        app.settings.chop_modifier,
                        app.settings.chop_ppq,
                        step.text()
                    ));
                }
                Ok(step) => {
                    if ui
                        .small_button(format!("Use 1/{} {} = {} ticks", app.settings.chop_denominator, app.settings.chop_modifier, step.text()))
                        .clicked()
                    {
                        app.fill_chop_from_note_value();
                    }
                }
                Err(e) => {
                    ui.colored_label(ERROR, e);
                }
            }
            ui.add_space(2.0);
            let ok = app.source.is_some() && app.chop_step().is_ok();
            if primary(ui, ok, "Chop").clicked() {
                app.request(Action::Chop);
            }
        },
    );
}

// ---- file dialogs -------------------------------------------------------------

fn dialog(app: &App) -> rfd::FileDialog {
    let mut d = rfd::FileDialog::new();
    if let Some(dir) = &app.settings.last_dir {
        d = d.set_directory(dir);
    }
    d
}

fn pick_dialog(app: &App) -> Option<PathBuf> {
    dialog(app)
        .add_filter("Domino or MIDI", &["dms", "dmp", "mid", "midi"])
        .add_filter("All files", &["*"])
        .pick_file()
}

pub fn open_file_dialog(app: &mut App) {
    if let Some(path) = pick_dialog(app) {
        app.request(Action::Open(path));
    }
}

fn save_dialog(app: &App, kind: FileKind, stem: &str) -> Option<PathBuf> {
    let ext = kind.extension();
    let path = dialog(app)
        .add_filter(kind.label(), &[ext])
        .set_file_name(format!("{stem}.{ext}"))
        .save_file()?;
    // keep the chosen format even if the extension was typed differently
    Some(if FileKind::from_path(&path) == Some(kind) { path } else { path.with_extension(ext) })
}
