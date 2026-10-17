//! Read-only overview of the working copy: every track's notes on one
//! canvas, with the flip range and the chop grid drawn as a preview of what
//! the buttons will do, and a track list underneath.
//!
//! The view opens on the first 7680 ticks and can be zoomed (Ctrl + wheel,
//! or the buttons) and panned (wheel, drag, scroll bar). Notes are
//! rasterised into one texture, in parallel vertical stripes, and only when
//! the view or the clip changes. Overlap is removed while rasterising: for
//! each track and key the stripe remembers how far right it has already
//! painted, so a note hidden under an earlier one costs one comparison and
//! no pixels. Each track's notes are indexed by length class (powers of 4),
//! so finding what a time window shows is a binary search per class that
//! looks back no further than that class's longest note.

use std::time::{Duration, Instant};

use eframe::egui::{self, Align2, Color32, ColorImage, FontId, Rect, RichText, Sense, Stroke, TextureHandle, TextureOptions};
use rayon::prelude::*;

use dominokit_core::format::clip::{Clip, Track};

use super::panels::key_label;
use super::state::{count, App};
use super::theme::{track_color, ACCENT};

/// Ticks shown when a clip is loaded.
pub const DEFAULT_VIEW: f64 = 7680.0;
const MIN_VIEW: f64 = 16.0;

/// One track's notes, grouped by length class: class `c` holds the notes
/// with `gate < 4^(c+1)`, as child indices in tick order.
struct TrackIndex {
    classes: Vec<(u64, Vec<usize>)>,
}

#[derive(Clone, Copy, PartialEq)]
struct RasterKey {
    generation: u64,
    start: f64,
    len: f64,
    size: [usize; 2],
    keys: (u8, u8),
}

pub struct Overview {
    start: f64,
    len: f64,
    load_id: u64,
    index: Option<(u64, Vec<TrackIndex>)>,
    texture: Option<TextureHandle>,
    key: Option<RasterKey>,
    last: (Duration, usize),
}

impl Default for Overview {
    fn default() -> Self {
        Overview { start: 0.0, len: DEFAULT_VIEW, load_id: u64::MAX, index: None, texture: None, key: None, last: (Duration::ZERO, 0) }
    }
}

pub fn show(view: &mut Overview, app: &mut App, ui: &mut egui::Ui) {
    let Some(stats) = app.stats.as_ref() else {
        empty_state(ui);
        return;
    };
    if app.source.is_none() {
        // busy on the worker: show the last picture, dimmed and still
        let height = (ui.available_height() * 0.62).clamp(180.0, 640.0);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), height), Sense::hover());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 6.0, ui.visuals().extreme_bg_color);
        if let Some(t) = &view.texture {
            let uv = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
            painter.image(t.id(), rect, uv, Color32::from_white_alpha(120));
        }
        ui.add_space(6.0);
        track_list(app, ui);
        return;
    }
    if view.load_id != app.load_id {
        view.load_id = app.load_id;
        view.start = 0.0;
        view.len = DEFAULT_VIEW;
    }
    let span = stats.span.max(1) as f64;
    let notes = stats.notes;
    let max_len = span.max(DEFAULT_VIEW) * 1.05;

    // ---- controls -----------------------------------------------------------
    ui.horizontal(|ui| {
        if ui.button("−").on_hover_text("Zoom out (Ctrl + wheel)").clicked() {
            view.len = (view.len * 2.0).min(max_len);
        }
        if ui.button("+").on_hover_text("Zoom in (Ctrl + wheel)").clicked() {
            view.len = (view.len / 2.0).max(MIN_VIEW);
        }
        if ui.button("7680").on_hover_text("Back to the default 7680-tick view").clicked() {
            view.len = DEFAULT_VIEW;
        }
        if ui.button("Whole clip").clicked() {
            view.start = 0.0;
            view.len = span.min(max_len);
        }
        ui.weak(format!("ticks {}–{} of {}", view.start as u64, (view.start + view.len) as u64, count(span as usize)));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.weak(format!("{} notes drawn in {:.1} ms", count(view.last.1), view.last.0.as_secs_f64() * 1e3))
                .on_hover_text("Notes hidden behind others, or outside the view, cost (almost) nothing.");
        });
    });

    if notes == 0 {
        ui.weak("The working copy has no notes.");
    } else {
        let height = (ui.available_height() * 0.62).clamp(180.0, 640.0);
        canvas(view, app, ui, height, span, max_len);
    }
    ui.add_space(6.0);
    track_list(app, ui);
}

fn empty_state(ui: &mut egui::Ui) {
    ui.add_space(40.0);
    ui.vertical_centered(|ui| {
        ui.label(RichText::new("Nothing loaded yet").size(18.0).strong());
        ui.add_space(6.0);
        ui.weak("1. In Domino, select notes and press Ctrl+C.");
        ui.weak("2. Press “Read” (or open / drop a .dms or .mid file).");
        ui.weak("3. Flip or Chop, then “Write” and Ctrl+V in Domino.");
    });
}

fn canvas(view: &mut Overview, app: &App, ui: &mut egui::Ui, height: f32, span: f64, max_len: f64) {
    let clip = app.source.as_ref().unwrap();
    let stats = app.stats.as_ref().unwrap();
    let (lowest, highest) = stats.keys.unwrap_or((0, 127));
    // a little headroom above and below, at least an octave tall
    let pad = (12u8.saturating_sub(highest - lowest) / 2).max(1);
    let (key_lo, key_hi) = (lowest.saturating_sub(pad), highest.saturating_add(pad));

    let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), height), Sense::click_and_drag());

    // ---- zoom and pan -------------------------------------------------------
    if response.hovered() {
        let (zoom, scroll, ctrl) = ui.input(|i| (i.zoom_delta(), i.smooth_scroll_delta, i.modifiers.command));
        if zoom != 1.0 {
            let anchor = response.hover_pos().map_or(0.5, |p| ((p.x - rect.left()) / rect.width()) as f64);
            let at = view.start + anchor * view.len;
            view.len = (view.len / zoom as f64).clamp(MIN_VIEW, max_len);
            view.start = at - anchor * view.len;
        } else if !ctrl {
            let delta = if scroll.x != 0.0 { scroll.x } else { scroll.y };
            view.start -= delta as f64 / rect.width() as f64 * view.len;
        }
    }
    if response.dragged() {
        view.start -= response.drag_delta().x as f64 / rect.width() as f64 * view.len;
    }
    view.start = view.start.clamp(0.0, (span - view.len * 0.5).max(0.0));

    // ---- notes ----------------------------------------------------------------
    let ppp = ui.ctx().pixels_per_point();
    let size = [((rect.width() * ppp) as usize).max(1), ((rect.height() * ppp) as usize).max(1)];
    let key = RasterKey { generation: app.generation, start: view.start, len: view.len, size, keys: (key_lo, key_hi) };
    if view.key != Some(key) || view.texture.is_none() {
        if view.index.as_ref().map(|(g, _)| *g) != Some(app.generation) {
            view.index = Some((app.generation, build_index(clip)));
        }
        let started = Instant::now();
        let (image, drawn) = rasterize(clip, &view.index.as_ref().unwrap().1, view.start, view.len, size, (key_lo, key_hi));
        match &mut view.texture {
            Some(t) => t.set(image, TextureOptions::NEAREST),
            None => view.texture = Some(ui.ctx().load_texture("overview", image, TextureOptions::NEAREST)),
        }
        view.key = Some(key);
        view.last = (started.elapsed(), drawn);
    }

    let painter = ui.painter_at(rect);
    let visuals = ui.visuals();
    painter.rect_filled(rect, 6.0, visuals.extreme_bg_color);
    let keys = (key_hi - key_lo) as f32 + 1.0;
    let y_of = |k: f32| rect.top() + (key_hi as f32 + 1.0 - k) / keys * rect.height();
    let x_of = |tick: f64| rect.left() + ((tick - view.start) / view.len) as f32 * rect.width();

    // flip range: a faint band, its edges, and the mirror axis (dashed)
    let range = app.flip_range();
    let (b0, b1) = (range.lo as f32, range.hi as f32 + 1.0);
    let (vis0, vis1) = (b0.max(key_lo as f32), b1.min(key_hi as f32 + 1.0));
    if vis1 > vis0 {
        painter.rect_filled(Rect::from_x_y_ranges(rect.x_range(), y_of(vis1)..=y_of(vis0)), 0.0, ACCENT.linear_multiply(0.03));
        let edge = Stroke::new(1.0_f32, ACCENT.linear_multiply(0.6));
        for k in [b0, b1] {
            if k > key_lo as f32 && k < key_hi as f32 + 1.0 {
                painter.hline(rect.x_range(), y_of(k), edge);
            }
        }
        let mid = (b0 + b1) / 2.0;
        if mid > key_lo as f32 && mid < key_hi as f32 + 1.0 {
            let y = y_of(mid);
            let axis = Stroke::new(1.0_f32, ACCENT.linear_multiply(0.4));
            painter.extend(egui::Shape::dashed_line(&[egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)], axis, 6.0, 4.0));
        }
    }

    // chop grid (only the lines in view)
    let mut grid_note = None;
    if let Ok(step) = app.chop_step() {
        let spacing = step.as_f64() / view.len * rect.width() as f64;
        if spacing >= 4.0 {
            let stroke = Stroke::new(1.0_f32, visuals.weak_text_color().linear_multiply(0.3));
            let mut i = (view.start / step.as_f64()).floor().max(0.0) as u64;
            loop {
                let t = step.line(i) as f64;
                if t > view.start + view.len {
                    break;
                }
                if t >= view.start {
                    painter.vline(x_of(t), rect.y_range(), stroke);
                }
                i += 1;
            }
        } else {
            grid_note = Some("chop grid too fine to draw at this zoom");
        }
    }

    if let Some(t) = &view.texture {
        painter.image(t.id(), rect, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
    }

    // past the end of the clip
    let end_x = x_of(span);
    if end_x < rect.right() {
        painter.rect_filled(Rect::from_x_y_ranges(end_x.max(rect.left())..=rect.right(), rect.y_range()), 0.0, Color32::from_black_alpha(90));
    }

    // corner labels on a backing so notes don't hide them
    let label = |pos: egui::Pos2, align: Align2, text: String| {
        let galley = painter.layout_no_wrap(text, FontId::proportional(11.0), visuals.text_color());
        let r = align.anchor_size(pos, galley.size()).expand2(egui::vec2(4.0, 1.0));
        painter.rect_filled(r, 3.0, visuals.extreme_bg_color.linear_multiply(0.85));
        painter.galley(r.min + egui::vec2(4.0, 1.0), galley, visuals.text_color());
    };
    label(rect.left_top() + egui::vec2(6.0, 5.0), Align2::LEFT_TOP, format!("key {} · {}", highest as u16 + 1, key_label(highest as u16 + 1)));
    label(rect.left_bottom() + egui::vec2(6.0, -5.0), Align2::LEFT_BOTTOM, format!("key {} · {}", lowest as u16 + 1, key_label(lowest as u16 + 1)));
    if let Some(msg) = grid_note {
        label(rect.right_top() + egui::vec2(-6.0, 5.0), Align2::RIGHT_TOP, msg.to_string());
    }

    if let Some(p) = response.hover_pos() {
        let tick = view.start + ((p.x - rect.left()) / rect.width()) as f64 * view.len;
        let key = (key_hi as f32 + 1.0 - (p.y - rect.top()) / rect.height() * keys).floor().max(0.0) as u16;
        response.on_hover_text_at_pointer(format!("tick {}\nkey {} ({})", tick.max(0.0) as u64, key + 1, key_label(key + 1)));
    }

    // scroll bar
    let bar_max = (span - view.len * 0.5).max(0.0);
    if bar_max > 0.0 {
        let mut start = view.start;
        ui.spacing_mut().slider_width = ui.available_width();
        ui.add(egui::Slider::new(&mut start, 0.0..=bar_max).show_value(false));
        view.start = start;
    }
    ui.horizontal(|ui| {
        legend(ui, ACCENT.linear_multiply(0.5), "flip range");
        legend(ui, Color32::GRAY, "chop grid");
        ui.weak("· Ctrl + wheel: zoom · wheel / drag: scroll");
    });
}

fn legend(ui: &mut egui::Ui, color: Color32, text: &str) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), Sense::hover());
    ui.painter().rect_filled(r, 2.0, color);
    ui.weak(text);
}

// ---- rasterising ------------------------------------------------------------------

fn build_index(clip: &Clip) -> Vec<TrackIndex> {
    let tracks: Vec<&Track> = clip.tracks().collect();
    tracks
        .par_iter()
        .map(|t| {
            let mut classes: Vec<Vec<usize>> = vec![Vec::new(); 17];
            for (i, c) in t.children.iter().enumerate() {
                if let Some(n) = c.as_note() {
                    classes[(n.gate.max(1).ilog2() / 2) as usize].push(i);
                }
            }
            let tick = |i: &usize| t.children[*i].as_note().unwrap().tick;
            let classes = classes
                .into_iter()
                .enumerate()
                .filter(|(_, v)| !v.is_empty())
                .map(|(c, mut v)| {
                    if !v.windows(2).all(|w| tick(&w[0]) <= tick(&w[1])) {
                        v.par_sort_by_key(tick);
                    }
                    (4u64.pow(c as u32 + 1), v)
                })
                .collect();
            TrackIndex { classes }
        })
        .collect()
}

/// Draw the notes in `[start, start + len)` into a `size` image, keys
/// `keys.0..=keys.1` from bottom to top. Returns the image and how many
/// notes actually painted pixels.
fn rasterize(clip: &Clip, index: &[TrackIndex], start: f64, len: f64, size: [usize; 2], keys: (u8, u8)) -> (ColorImage, usize) {
    let [w, h] = size;
    let tracks: Vec<&Track> = clip.tracks().collect();
    let tpp = len / w as f64; // ticks per pixel
    let rows = (keys.1 - keys.0) as f64 + 1.0;
    // pixel rows of each key: top row, and bottom row (exclusive)
    let row_of: Vec<(usize, usize)> = (0..=255u16)
        .map(|k| {
            if k < keys.0 as u16 || k > keys.1 as u16 {
                return (0, 0);
            }
            let from_top = (keys.1 as u16 - k) as f64;
            let y0 = ((from_top / rows * h as f64).floor() as usize).min(h - 1);
            let y1 = (((from_top + 1.0) / rows * h as f64).floor() as usize).clamp(y0 + 1, h);
            (y0, y1)
        })
        .collect();

    let stripes = (w / 48).clamp(1, rayon::current_num_threads() * 3);
    let bounds: Vec<usize> = (0..=stripes).map(|s| s * w / stripes).collect();
    let painted: Vec<(Vec<Color32>, usize)> = (0..stripes)
        .into_par_iter()
        .map(|s| {
            let (x0, x1) = (bounds[s], bounds[s + 1]);
            let sw = x1 - x0;
            let mut px = vec![Color32::TRANSPARENT; sw * h];
            let (t0, t1) = (start + x0 as f64 * tpp, start + x1 as f64 * tpp);
            let mut drawn = 0usize;
            for (ti, (track, idx)) in tracks.iter().zip(index).enumerate() {
                let base = track_color(ti);
                for (longest, notes) in &idx.classes {
                    // how far right each key is already painted (per class:
                    // within one class the notes come in tick order)
                    let mut covered = [0usize; 256];
                    let from_tick = (t0 - *longest as f64).max(0.0);
                    let first = notes.partition_point(|&i| (track.children[i].as_note().unwrap().tick as f64) < from_tick);
                    for &ci in &notes[first..] {
                        let n = track.children[ci].as_note().unwrap();
                        let tick = n.tick as f64;
                        if tick >= t1 {
                            break;
                        }
                        let end = n.end_u32() as f64;
                        if end <= t0 || n.key < keys.0 || n.key > keys.1 {
                            continue;
                        }
                        let a = (((tick - start) / tpp).floor() as i64 - x0 as i64).max(0) as usize;
                        let b = (((end - start) / tpp).ceil() as i64 - x0 as i64).clamp(a as i64 + 1, sw as i64) as usize;
                        let c = &mut covered[n.key as usize];
                        if b <= *c {
                            continue; // hidden behind what this key already shows
                        }
                        let from = a.max(*c);
                        *c = b;
                        drawn += 1;
                        let shade = 0.55 + 0.45 * (n.vel.min(127) as f32 / 127.0);
                        let color = Color32::from_rgb((base.r() as f32 * shade) as u8, (base.g() as f32 * shade) as u8, (base.b() as f32 * shade) as u8);
                        let (y0, y1) = row_of[n.key as usize];
                        for y in y0..y1 {
                            px[y * sw + from..y * sw + b].fill(color);
                        }
                    }
                }
            }
            (px, drawn)
        })
        .collect();

    let mut image = ColorImage::new(size, Color32::TRANSPARENT);
    let mut drawn = 0;
    for (s, (px, d)) in painted.iter().enumerate() {
        drawn += d;
        let (x0, x1) = (bounds[s], bounds[s + 1]);
        let sw = x1 - x0;
        for y in 0..h {
            image.pixels[y * w + x0..y * w + x1].copy_from_slice(&px[y * sw..(y + 1) * sw]);
        }
    }
    (image, drawn)
}

// ---- track list -------------------------------------------------------------------

fn track_list(app: &App, ui: &mut egui::Ui) {
    let Some(stats) = &app.stats else { return };
    ui.label(RichText::new(format!("Tracks ({})", count(stats.tracks.len()))).strong());
    let widths = [36.0, 220.0, 40.0, 110.0, 110.0, 80.0];
    let header = ["#", "Name", "Ch", "Notes", "Other events", "Keys"];
    ui.horizontal(|ui| {
        for (h, w) in header.iter().zip(widths) {
            ui.add_sized([w, 16.0], egui::Label::new(RichText::new(*h).weak()));
        }
    });
    let row_h = 20.0;
    egui::ScrollArea::vertical().id_source("tracks").auto_shrink([false, true]).show_rows(ui, row_h, stats.tracks.len(), |ui, rows| {
        for i in rows {
            let t = &stats.tracks[i];
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(egui::vec2(widths[0], 14.0), Sense::hover());
                ui.painter().rect_filled(r.shrink2(egui::vec2(4.0, 1.0)), 2.0, track_color(i));
                ui.painter().text(r.center(), Align2::CENTER_CENTER, (i + 1).to_string(), FontId::proportional(11.0), Color32::BLACK);
                let name = if t.conductor {
                    let label = if t.name.is_empty() || t.name.eq_ignore_ascii_case("conductor") {
                        "Conductor".to_string()
                    } else {
                        format!("{} · conductor", t.name)
                    };
                    RichText::new(label).italics()
                } else if t.name.is_empty() {
                    RichText::new("—").weak()
                } else {
                    RichText::new(t.name.clone())
                };
                ui.add_sized([widths[1], row_h], egui::Label::new(name).truncate(true));
                ui.add_sized([widths[2], row_h], egui::Label::new(t.channel.map(|c| (c as u16 + 1).to_string()).unwrap_or("—".into())));
                ui.add_sized([widths[3], row_h], egui::Label::new(count(t.notes)));
                ui.add_sized([widths[4], row_h], egui::Label::new(count(t.events)));
                let keys = t.keys.map(|(lo, hi)| format!("{}–{}", lo as u16 + 1, hi as u16 + 1)).unwrap_or("—".into());
                ui.add_sized([widths[5], row_h], egui::Label::new(keys));
            });
        }
    });
}
