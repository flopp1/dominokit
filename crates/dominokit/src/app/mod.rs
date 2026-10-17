//! GUI application.
//!
//! The window only draws; every action runs on a worker thread (see
//! [`jobs`]). While one runs, the window shows a read-only stand-in of the
//! state with its controls disabled, and after a second a progress window.

pub mod inspector;
pub mod jobs;
pub mod overview;
pub mod panels;
pub mod state;
pub mod stats;
pub mod theme;

use std::time::Duration;

use eframe::egui;

use jobs::{Action, Job};
use state::App;

/// Work shorter than this finishes without a progress window.
const MODAL_AFTER: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Overview,
    Inspector,
}

pub struct DominoKitApp {
    /// The real state; `None` while it is on the worker thread.
    app: Option<App>,
    /// What the window shows while the real state is busy.
    stand_in: Option<App>,
    job: Option<Job>,
    pub tab: Tab,
    overview: overview::Overview,
    inspector: inspector::Inspector,
    styled: bool,
    last_window_size: Option<[f32; 2]>,
}

impl DominoKitApp {
    pub fn new(app: App) -> Self {
        DominoKitApp {
            app: Some(app),
            stand_in: None,
            job: None,
            tab: Tab::Overview,
            overview: overview::Overview::default(),
            inspector: inspector::Inspector::default(),
            styled: false,
            last_window_size: None,
        }
    }

    fn busy(&self) -> bool {
        self.job.is_some()
    }

    fn handle_keys(&mut self, ctx: &egui::Context, app: &mut App) {
        // shortcuts act only while DominoKit has focus; they never touch Domino
        let pressed = |key| ctx.input(|i| i.modifiers.command && i.key_pressed(key));
        if pressed(egui::Key::R) {
            app.request(Action::Read);
        }
        if pressed(egui::Key::W) {
            app.request(Action::Write);
        }
        if pressed(egui::Key::Z) {
            app.request(Action::Undo);
        }
        if pressed(egui::Key::O) {
            panels::open_file_dialog(app);
        }
        // a .dms / .mid dropped onto the window opens it
        let dropped: Vec<_> = ctx.input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect());
        if let Some(path) = dropped.into_iter().next() {
            app.request(Action::Open(path));
        }
    }

    /// Take back a finished job's state, or start the next requested action.
    fn pump(&mut self, ctx: &egui::Context) {
        if self.job.as_ref().is_some_and(Job::is_finished) {
            let job = self.job.take().unwrap();
            let settings = self.stand_in.as_ref().map(|s| s.settings.clone());
            let app = job.join(|| {
                let clipboard = Box::new(crate::clipboard::platform_clipboard());
                App::new(clipboard, settings.unwrap_or_default())
            });
            self.app = Some(app);
            self.stand_in = None;
        }
        if self.job.is_none() {
            let Some(app) = self.app.as_mut() else { return };
            if app.requests.is_empty() {
                return;
            }
            let action = app.requests.remove(0);
            let app = self.app.take().unwrap();
            self.stand_in = Some(app.stand_in(format!("{}…", action.title())));
            let ctx = ctx.clone();
            self.job = Some(Job::start(app, action, move || ctx.request_repaint()));
        }
    }

    fn progress_window(&self, ctx: &egui::Context) {
        let Some(job) = &self.job else { return };
        let elapsed = job.started.elapsed();
        // keep the clock and the bar moving
        ctx.request_repaint_after(Duration::from_millis(50));
        if elapsed < MODAL_AFTER {
            return;
        }
        // dim everything behind the window
        let screen = ctx.screen_rect();
        ctx.layer_painter(egui::LayerId::new(egui::Order::PanelResizeLine, egui::Id::new("busy_dim")))
            .rect_filled(screen, 0.0, egui::Color32::from_black_alpha(110));
        egui::Window::new(&job.title)
            .id(egui::Id::new("progress_window"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .fixed_size([360.0, 0.0])
            .show(ctx, |ui| {
                let stage = job.progress.get();
                ui.label(if stage.is_empty() { "Finishing…".to_string() } else { format!("{stage}…") });
                ui.add(egui::ProgressBar::new(((elapsed.as_secs_f32() * 0.6).sin() * 0.5 + 0.5).clamp(0.02, 1.0)).animate(true));
                ui.weak(format!("{:.1} s — the window stays usable; the work runs in the background.", elapsed.as_secs_f32()));
            });
    }
}

impl eframe::App for DominoKitApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if !self.styled {
            theme::apply(ctx);
            self.styled = true;
        }
        self.pump(ctx);

        let busy = self.busy();
        let mut app = match (self.app.take(), self.stand_in.take()) {
            (Some(app), stand_in) => {
                self.stand_in = stand_in;
                app
            }
            (None, Some(stand_in)) => stand_in,
            (None, None) => unreachable!("either the app or its stand-in is here"),
        };
        if !busy {
            self.handle_keys(ctx, &mut app);
        }

        panels::header(&mut app, ctx, busy);
        panels::status_bar(&app, ctx);
        panels::side_panel(&mut app, ctx, busy);
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.tab, Tab::Overview, "Overview");
                ui.selectable_value(&mut self.tab, Tab::Inspector, "Inspector");
            });
            ui.separator();
            match self.tab {
                Tab::Overview => overview::show(&mut self.overview, &mut app, ui),
                Tab::Inspector => inspector::show(&mut self.inspector, &mut app, ui),
            }
        });

        // put the state back where it came from
        if busy {
            self.stand_in = Some(app);
        } else {
            self.app = Some(app);
        }
        self.progress_window(ctx);
        // a request made this frame starts right away
        self.pump(ctx);

        let size = ctx.screen_rect().size();
        self.last_window_size = Some([size.x, size.y]);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if let Some(job) = self.job.take() {
            // let the running action finish so settings and files stay whole
            self.app = Some(job.join(|| App::new(Box::new(crate::clipboard::platform_clipboard()), Default::default())));
        }
        if let Some(app) = &mut self.app {
            if let Some(size) = self.last_window_size {
                app.settings.window_size = Some(size);
            }
            app.settings.save();
        }
    }
}
