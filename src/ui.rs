use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Stroke, Vec2};

use flashmob::model::Project;
use flashmob::sld;
use flashmob::study::{self, Studies, StudyOutput};
use flashmob::tcc::{self, TccPlot};

pub fn launch(project: Option<std::path::PathBuf>) -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1360.0, 860.0])
            .with_min_inner_size([1020.0, 680.0])
            .with_title("Flashmob"),
        ..Default::default()
    };
    eframe::run_native(
        "Flashmob",
        options,
        Box::new(|cc| {
            let mut style = (*cc.egui_ctx.global_style()).clone();
            style.visuals = terminal_visuals();
            for font in style.text_styles.values_mut() {
                font.family = egui::FontFamily::Monospace;
            }
            cc.egui_ctx.set_global_style(style);
            Ok(Box::new(App::new(project)))
        }),
    )
}

struct App {
    project: Project,
    results: Option<StudyOutput>,
    tab: Tab,
    selected: Option<String>,
    status: String,
    show_warnings: bool,
    zoom: f32,
    /// Top-left of the diagram, in points inside the fixed one-line panel.
    pan: Vec2,
    fit_view: bool,
    tcc_kv: f64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Sld,
    Flow,
    Fault,
    Arc,
    Tcc,
}

impl App {
    fn new(path: Option<std::path::PathBuf>) -> Self {
        let opened = path.as_ref().and_then(|path| {
            std::fs::read_to_string(path)
                .map_err(|err| err.to_string())
                .and_then(|text| flashmob::exec::load_project(&text))
                .ok()
                .map(|project| (path.clone(), project))
        });
        let failed = path.filter(|_| opened.is_none());
        let mut app = Self {
            project: Project::sample(),
            results: None,
            tab: Tab::Sld,
            selected: None,
            status: "Sample plant is loaded. Short circuit and arc flash are on the one-line.".into(),
            show_warnings: false,
            zoom: 1.0,
            pan: Vec2::ZERO,
            fit_view: true,
            tcc_kv: 0.48,
        };
        if let Some((path, project)) = opened {
            app.project = project;
            app.run_study();
            app.status = format!("Opened {}. {}", path.display(), app.status);
        } else if let Some(path) = failed {
            app.status = format!("Could not open {}", path.display());
            app.run_study();
        } else {
            app.run_study();
        }
        app
    }

    fn run_study(&mut self) {
        self.results = None;
        match study::run(&self.project, Studies::all()) {
            Ok(results) => {
                let ms = results.elapsed_ms;
                let n = results.warnings.len();
                self.status = format!("Ran in {ms:.1} ms. {n} warnings.");
                if n == 0 {
                    self.show_warnings = false;
                }
                self.results = Some(results);
            }
            Err(err) => self.status = err,
        }
    }

    fn warnings_window(&mut self, ctx: &egui::Context) {
        if !self.show_warnings {
            return;
        }
        let warnings = self.results.as_ref().map(|r| r.warnings.clone()).unwrap_or_default();
        if warnings.is_empty() {
            self.show_warnings = false;
            return;
        }
        let mut open = true;
        egui::Window::new("WARNINGS")
            .anchor(Align2::LEFT_BOTTOM, [8.0, -40.0])
            .collapsible(false)
            .resizable(true)
            .default_width(560.0)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.label(RichText::new(format!("{} ITEM{}", warnings.len(), if warnings.len() == 1 { "" } else { "S" })).color(color("#FFB300")));
                ui.separator();
                egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                    for warning in &warnings {
                        ui.label(RichText::new(warning.to_uppercase()).color(color("#E2E8F0")));
                        ui.add_space(6.0);
                    }
                });
            });
        self.show_warnings = open;
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if ui.ctx().input(|i| i.modifiers.command && i.key_pressed(egui::Key::R)) {
            self.run_study();
        }
        egui::Panel::top("top").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("FLASHMOB").size(16.0).strong().color(color("#E2E8F0")));
                ui.label(RichText::new(self.project.name.to_uppercase()).size(14.0).color(color("#94A3B8")));
                if !self.project.assumptions.is_empty() {
                    ui.label(RichText::new("PRELIMINARY - ASSUMPTIONS APPLY").color(color("#FFB300")))
                        .on_hover_text(self.project.assumptions.iter().map(|a| format!("{}: {}", a.id, a.statement)).collect::<Vec<_>>().join("\n\n"));
                }
                ui.separator();
                if ui.button("Run").clicked() {
                    self.run_study();
                }
                if ui.button("Sample").clicked() {
                    self.project = Project::sample();
                    self.selected = None;
                    self.fit_view = true;
                    self.run_study();
                }
                if ui.button("Open").clicked() {
                    self.open();
                }
                if ui.button("Save").clicked() {
                    self.save();
                }
                if ui.button("Export SLD").clicked() {
                    self.export_sld();
                }
                if ui.button("Export TCC").clicked() {
                    self.export_tcc();
                }
                if ui.button("Export SKM").clicked() {
                    self.export_skm();
                }
            });
            ui.horizontal(|ui| {
                for (tab, label) in [
                    (Tab::Sld, "One-line"),
                    (Tab::Flow, "Load flow"),
                    (Tab::Fault, "Short circuit"),
                    (Tab::Arc, "Arc flash"),
                    (Tab::Tcc, "TCC"),
                ] {
                    ui.selectable_value(&mut self.tab, tab, label);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new("IEEE 1584-2018").color(color("#00FF66")));
                });
            });
        });
        egui::Panel::bottom("status").show(ui, |ui| {
            ui.horizontal(|ui| {
                let n = self.results.as_ref().map(|r| r.warnings.len()).unwrap_or(0);
                if n > 0 {
                    let label = format!("{n} WARNING{}", if n == 1 { "" } else { "S" });
                    let text = RichText::new(label).strong().color(color("#FFB300"));
                    if ui.button(text).clicked() {
                        self.show_warnings = !self.show_warnings;
                    }
                }
                ui.label(RichText::new(self.status.to_uppercase()).color(color("#94A3B8")));
            });
        });
        self.warnings_window(ui.ctx());
        egui::Panel::right("inspector").default_size(300.0).show(ui, |ui| {
            self.inspector(ui);
        });
        egui::CentralPanel::default().show(ui, |ui| match self.tab {
            Tab::Sld => self.sld(ui),
            Tab::Flow => self.flow(ui),
            Tab::Fault => self.fault(ui),
            Tab::Arc => self.arc(ui),
            Tab::Tcc => self.tcc(ui),
        });
    }
}

impl App {
    fn sld(&mut self, ui: &mut egui::Ui) {
        let Some(results) = self.results.clone() else {
            ui.label("Run the study to draw the one-line.");
            return;
        };
        let diagram = sld::diagram(&self.project, &results);
        let old_zoom = self.zoom;
        let mut slider_changed = false;
        ui.horizontal(|ui| {
            ui.label("Zoom");
            slider_changed = ui.add(egui::Slider::new(&mut self.zoom, 0.05..=8.0).logarithmic(true)).changed();
            if ui.button("Fit").clicked() { self.fit_view = true; }
            ui.label(match diagram_detail(self.zoom) {
                DiagramDetail::Full => "Full detail",
                DiagramDetail::Compact => "Summary · hover or select a bus for details",
                DiagramDetail::Overview => "Overview · hover or select a bus for details",
            });
        });
        let view = ui.available_rect_before_wrap();
        let (rect, response) = ui.allocate_exact_size(view.size(), Sense::click_and_drag());
        if slider_changed {
            self.pan = zoom_pan(self.pan, rect.size() * 0.5, old_zoom, self.zoom);
        }
        if self.fit_view && diagram.width > 1.0 && diagram.height > 1.0 {
            let fit = (rect.width() / diagram.width as f32).min(rect.height() / diagram.height as f32);
            self.zoom = fit.clamp(0.05, 8.0);
            let content = Vec2::new(diagram.width as f32, diagram.height as f32) * self.zoom;
            self.pan = (rect.size() - content) * 0.5;
            self.fit_view = false;
        }
        let zoom_delta = ui.input(|i| i.zoom_delta());
        if response.hovered() && (zoom_delta - 1.0).abs() > 0.001 {
            if let Some(pos) = ui.input(|i| i.pointer.hover_pos()) {
                let new = (self.zoom * zoom_delta).clamp(0.05, 8.0);
                self.pan = zoom_pan(self.pan, pos - rect.min, self.zoom, new);
                self.zoom = new;
            }
        } else if response.hovered() {
            // A pinch can also emit scroll events; do not apply those as a second pan.
            self.pan += ui.input(|i| i.smooth_scroll_delta);
        }
        if response.dragged() {
            self.pan += ui.input(|i| i.pointer.delta());
        }
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, color("#0A0C10"));
        let origin = rect.min + self.pan;
        let z = self.zoom;
        let map = |x: f64, y: f64| origin + Vec2::new(x as f32, y as f32) * z;
        draw_diagram_grid(&painter, rect, origin, z);
        let ink = color("#E2E8F0");
        for branch in &diagram.branches {
            let stroke = Stroke::new((2.0 * z).clamp(0.8, 2.0), ink);
            let pts = branch.points();
            for pair in pts.windows(2) {
                painter.line_segment([map(pair[0].0, pair[0].1), map(pair[1].0, pair[1].1)], stroke);
            }
            let (wx, wy) = branch.winding_at();
            if branch.transformer {
                painter.circle_stroke(map(wx, wy - 10.0), 12.0 * z, stroke);
                painter.circle_stroke(map(wx, wy + 10.0), 12.0 * z, stroke);
            }
            if branch.protector != "none" {
                let (dx, dy) = branch.device_at();
                let h = if branch.protector == "switch" { 10.0 } else { 14.0 };
                let box_rect = Rect::from_center_size(map(dx, dy), Vec2::new(14.0, h) * z);
                painter.rect_stroke(box_rect, 0.0, stroke, egui::StrokeKind::Inside);
            }
            if diagram_detail(z) == DiagramDetail::Full {
                let label = Rect::from_min_size(map(wx + 20.0, wy - 8.0), Vec2::new(140.0, 18.0) * z);
                if !diagram.buses.iter().any(|bus| label.intersects(bus_card_at(bus, origin, z))) {
                    paint_diagram_line(&painter, label, &branch.name.to_uppercase(), 10.0 * z, color("#94A3B8"));
                }
            }
        }
        let hovered = response.hover_pos().and_then(|pointer| {
            diagram.buses.iter().filter(|bus| bus_hit_rect(bus, origin, z).contains(pointer))
                .min_by(|a, b| {
                    let distance = |bus: &sld::BusGlyph| map(bus.x, bus.y).distance_sq(pointer);
                    distance(a).total_cmp(&distance(b))
                })
        });
        if response.clicked() { self.selected = hovered.map(|bus| bus.id.clone()); }
        for bus in &diagram.buses {
            draw_bus(&painter, bus, origin, z, self.selected.as_deref() == Some(&bus.id));
        }
        if let Some(bus) = hovered {
            response.on_hover_ui_at_pointer(|ui| {
                ui.strong(&bus.name);
                ui.label(format!("{:.3} kV · 3P {} · LG {}", bus.kv, ka(bus.fault_3p_ka), ka(bus.fault_lg_ka)));
                for arc in &bus.arcs {
                    ui.label(format!("{}: {:.2} cal/cm² · AFB {:.1} in · {:.3} s", arc.name, arc.cal_cm2, arc.afb_in, arc.time_s));
                }
                for failure in &bus.arc_failures { ui.label(format!("Arc flash FAILED: {failure}")); }
                ui.weak("Click to inspect this bus.");
            });
        }
    }

    fn flow(&self, ui: &mut egui::Ui) {
        let Some(lf) = self.results.as_ref().and_then(|r| r.loadflow.as_ref()) else {
            ui.label("No load-flow result.");
            return;
        };
        ui.label(format!(
            "{} in {} iterations, mismatch {:.2e} pu",
            if lf.converged { "Converged" } else { "Did not converge" },
            lf.iterations,
            lf.max_mismatch_pu
        ));
        if !lf.converged { return; }
        egui::Grid::new("flow").striped(true).show(ui, |ui| {
            ui.label(RichText::new("Bus").strong());
            ui.label(RichText::new("V pu").strong());
            ui.label(RichText::new("Angle").strong());
            ui.label(RichText::new("P MW").strong());
            ui.label(RichText::new("Q Mvar").strong());
            ui.end_row();
            for bus in &lf.buses {
                ui.label(&bus.name);
                ui.label(format!("{:.4}", bus.v_pu));
                ui.label(format!("{:.2}°", bus.angle_deg));
                ui.label(format!("{:.3}", bus.p_mw));
                ui.label(format!("{:.3}", bus.q_mvar));
                ui.end_row();
            }
        });
    }

    fn fault(&self, ui: &mut egui::Ui) {
        let Some(fault) = self.results.as_ref().and_then(|r| r.fault.as_ref()) else {
            ui.label("No short-circuit result.");
            return;
        };
        ui.label(format!("Prefault requested: {}; used: {}", fault.prefault_requested, fault.prefault));
        if !fault.valid { ui.label(fault.error.as_deref().unwrap_or("Invalid fault result")); return; }
        egui::Grid::new("fault").striped(true).show(ui, |ui| {
            for heading in ["Bus", "3P kA", "LG kA", "LL kA", "LLG kA", "X/R", "IEC peak"] {
                ui.label(RichText::new(heading).strong());
            }
            ui.end_row();
            for bus in &fault.buses {
                ui.label(&bus.name);
                ui.label(ka(bus.three_phase.as_ref().map(|p| p.symmetrical_ka)));
                ui.label(ka(bus.line_to_ground.as_ref().map(|p| p.symmetrical_ka)));
                ui.label(ka(bus.line_to_line.as_ref().map(|p| p.symmetrical_ka)));
                ui.label(ka(bus.line_to_line_ground.as_ref().map(|p| p.symmetrical_ka)));
                ui.label(bus.three_phase.as_ref().map(|p| p.x_over_r.map(|v| format!("{v:.1}")).unwrap_or_else(|| "∞".into())).unwrap_or_else(|| "—".into()));
                ui.label(ka(bus.three_phase.as_ref().map(|p| p.iec_peak_ka)));
                ui.end_row();
            }
        });
    }

    fn arc(&self, ui: &mut egui::Ui) {
        if let Some(out) = &self.results {
            for f in &out.arc_flash_failures { ui.colored_label(color("#FFB300"), format!("FAILED — {}: {}", f.name, f.error)); }
        }
        let Some(rows) = self.results.as_ref().and_then(|r| r.arc_flash.as_ref()) else {
            ui.label("No arc-flash result.");
            return;
        };
        ui.label("IEEE 1584-2018. The governing energy is the higher of the arcing-current case and the reduced-arcing-current case.");
        egui::Grid::new("arc").striped(true).show(ui, |ui| {
            for heading in ["Equipment", "Bus", "Bolted kA", "Arc kA", "s", "cal/cm²", "AFB in"] {
                ui.label(RichText::new(heading).strong());
            }
            ui.end_row();
            for row in rows {
                ui.label(&row.name);
                ui.label(&row.bus_name);
                ui.label(format!("{:.2}", row.bolted_ka));
                ui.label(format!("{:.2}", row.arcing_ka));
                let seconds = if row.governing == "reduced_arcing" { row.time_min_s } else { row.time_s };
                ui.label(format!("{seconds:.3}")).on_hover_text(&row.duration_note);
                ui.label(RichText::new(format!("{:.2}", row.governing_cal_cm2)).color(color(sld::energy_color(row.governing_cal_cm2))));
                ui.label(format!("{:.0}", row.afb_in));
                ui.end_row();
            }
        });
    }

    fn tcc(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Refer current to");
            ui.add(egui::DragValue::new(&mut self.tcc_kv).speed(0.01).range(0.05..=40.0).suffix(" kV"));
        });
        let Some(results) = self.results.clone() else {
            ui.label("Run the study before drawing curves.");
            return;
        };
        let plot = match tcc::plot(&self.project, Some(&results), Some(self.tcc_kv), &[]) {
            Ok(p) => p, Err(e) => { ui.label(e); return; }
        };
        egui::ScrollArea::both().show(ui, |ui| {
            let (rect, _) = ui.allocate_exact_size(Vec2::new(980.0, 680.0), Sense::hover());
            draw_tcc(ui, rect, &plot);
        });
    }

    fn inspector(&mut self, ui: &mut egui::Ui) {
        ui.heading("Bus");
        let Some(id) = self.selected.clone() else {
            ui.label("Click a bus on the one-line. Each card shows three-phase and line-to-ground fault current, then the governing arc-flash energy.");
            return;
        };
        let mut edited = false;
        let Some((name, kv)) = self.project.buses.iter_mut().find(|b| b.id == id).map(|bus| {
            ui.label(RichText::new(&bus.name).strong());
            ui.horizontal(|ui| {
                ui.label("kV");
                edited |= ui.add(egui::DragValue::new(&mut bus.kv).speed(0.01).range(0.05..=40.0)).changed();
            });
            (bus.name.clone(), bus.kv)
        }) else {
            ui.label("That bus is no longer in the project.");
            return;
        };
        if edited { self.results = None; self.status = "Results stale — model edited; run the study".into(); }
        if let Some(results) = &self.results {
            if let Some(protection) = results.protection.iter().find(|p| p.bus_id == id) {
                ui.separator();
                ui.label(RichText::new("Protection from topology").strong());
                ui.label(&protection.detail);
            }
            for f in results.arc_flash_failures.iter().filter(|f| f.bus_id == id) { ui.label(format!("Arc flash FAILED: {}", f.error)); }
            if let Some(fault) = results.fault.as_ref().and_then(|f| f.buses.iter().find(|b| b.id == id)) {
                ui.separator();
                ui.label(RichText::new("Short circuit").strong());
                if let Some(p) = &fault.three_phase {
                    ui.label(format!("3P  {:.2} kA   X/R {}   peak {:.2} kA", p.symmetrical_ka, p.x_over_r.map(|v| format!("{v:.1}")).unwrap_or_else(|| "∞".into()), p.iec_peak_ka));
                }
                if let Some(p) = &fault.line_to_ground {
                    ui.label(format!("LG  {:.2} kA", p.symmetrical_ka));
                }
            }
            if let Some(rows) = &results.arc_flash {
                for row in rows.iter().filter(|r| r.bus_id == id) {
                    ui.separator();
                    ui.label(RichText::new(&row.name).strong().color(color(sld::energy_color(row.governing_cal_cm2))));
                    ui.label(format!("{:.2} cal/cm²    AFB {:.0} in", row.governing_cal_cm2, row.afb_in));
                    ui.label(format!("Arcing {:.2} / {:.2} kA", row.arcing_ka, row.arcing_min_ka));
                    ui.label(&row.duration_note);
                    ui.label(format!("IEEE 1584-2018  {}", row.electrode));
                    if row.assumed {
                        ui.label("Assumed inputs: see duration note and equipment record.");
                    }
                }
            }
        }
        ui.separator();
        ui.label(RichText::new(format!("{name}  {kv:.3} kV")).weak());
        ui.label("Change the voltage, then Run. Export SLD writes the same diagram as SVG.");
    }

    fn open(&mut self) {
        if let Some(path) = rfd::FileDialog::new().add_filter("Flashmob project", &["json"]).pick_file() {
            match std::fs::read_to_string(&path).map_err(|e| e.to_string()).and_then(|t| flashmob::exec::load_project(&t)) {
                Ok(project) => {
                    self.project = project;
                    self.selected = None;
                    self.fit_view = true;
                    self.run_study();
                }
                Err(err) => self.status = err,
            }
        }
    }

    fn save(&mut self) {
        if let Some(path) = rfd::FileDialog::new().set_file_name("project.json").save_file() {
            match serde_json::to_string_pretty(&self.project) {
                Ok(text) => {
                    if let Err(err) = std::fs::write(&path, text) {
                        self.status = err.to_string();
                    } else {
                        self.status = format!("Saved {}", path.display());
                    }
                }
                Err(err) => self.status = err.to_string(),
            }
        }
    }

    fn export_sld(&mut self) {
        if self.results.is_none() {
            self.run_study();
        }
        let Some(results) = &self.results else { return };
        if let Some(path) = rfd::FileDialog::new().set_file_name("flashmob-sld.svg").save_file() {
            let svg = sld::to_svg(&sld::diagram(&self.project, results));
            self.status = write_status(&path, &svg);
        }
    }

    fn export_skm(&mut self) {
        if let Some(path) = rfd::FileDialog::new().pick_folder() {
            match flashmob::skm::write_dir(&self.project, &path) {
                Ok(dir) => self.status = format!("SKM export written to {}", dir.display()),
                Err(err) => self.status = err,
            }
        }
    }

    fn export_tcc(&mut self) {
        if self.results.is_none() {
            self.run_study();
        }
        let Some(results) = &self.results else { return };
        let plot = match tcc::plot(&self.project, Some(results), Some(self.tcc_kv), &[]) {
            Ok(p) => p, Err(e) => { self.status = e; return; }
        };
        if let Some(path) = rfd::FileDialog::new().set_file_name("flashmob-tcc.svg").save_file() {
            self.status = write_status(&path, &tcc::to_svg(&plot));
            let csv_path = path.with_extension("csv");
            if let Err(err) = std::fs::write(&csv_path, tcc::to_csv(&plot)) {
                self.status = err.to_string();
            } else {
                self.status = format!("Wrote {} and {}", path.display(), csv_path.display());
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DiagramDetail { Overview, Compact, Full }

fn diagram_detail(zoom: f32) -> DiagramDetail {
    if zoom < 0.4 { DiagramDetail::Overview }
    else if zoom < 0.95 { DiagramDetail::Compact }
    else { DiagramDetail::Full }
}

fn zoom_pan(pan: Vec2, anchor: Vec2, old: f32, new: f32) -> Vec2 {
    anchor - (anchor - pan) * (new / old)
}

/// The minor grid remains 24–48 screen points apart; alternating lines fade
/// away before the next coarser level takes over. Iterate only the viewport.
fn grid_lines(min: f32, max: f32, origin: f32, zoom: f32) -> Vec<(f32, f32)> {
    if ![min, max, origin, zoom].iter().all(|v| v.is_finite()) || zoom <= 0.0 || max <= min {
        return Vec::new();
    }
    let base = 40.0 * zoom;
    let spacing = base * 2.0_f32.powf((24.0 / base).log2().ceil());
    let first = min + (origin - min).rem_euclid(spacing);
    let parity = ((first - origin) / spacing).round().rem_euclid(2.0) as usize;
    let fade = ((spacing - 24.0) / 24.0).clamp(0.0, 1.0);
    let count = ((max - first) / spacing).floor().max(-1.0) as isize + 1;
    (0..count).map(|n| (first + n as f32 * spacing, if (n as usize + parity) % 2 == 0 { 1.0 } else { fade })).collect()
}

fn draw_diagram_grid(painter: &egui::Painter, rect: Rect, origin: Pos2, zoom: f32) {
    let grid = color("#334155");
    for (x, alpha) in grid_lines(rect.left(), rect.right(), origin.x, zoom) {
        let x = painter.round_to_pixel_center(x);
        painter.line_segment([Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())], Stroke::new(1.0, grid.gamma_multiply(alpha * 0.6)));
    }
    for (y, alpha) in grid_lines(rect.top(), rect.bottom(), origin.y, zoom) {
        let y = painter.round_to_pixel_center(y);
        painter.line_segment([Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)], Stroke::new(1.0, grid.gamma_multiply(alpha * 0.6)));
    }
}

/// Ellipsis and a local clip keep even very long equipment names inside their allotted row.
fn paint_diagram_line(painter: &egui::Painter, rect: Rect, text: &str, size: f32, ink: Color32) {
    if rect.width() <= 0.0 || rect.height() <= 0.0 { return; }
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), FontId::monospace(size), ink);
    job.wrap = egui::text::TextWrapping::truncate_at_width(rect.width());
    let galley = painter.layout_job(job);
    painter.with_clip_rect(rect.intersect(painter.clip_rect())).galley(rect.min, galley, ink);
}

fn draw_bus(painter: &egui::Painter, bus: &sld::BusGlyph, origin: Pos2, z: f32, selected: bool) {
    let map = |x: f64, y: f64| origin + Vec2::new(x as f32, y as f32) * z;
    let ink = if selected { color("#00FF66") } else { color("#E2E8F0") };
    let stroke = Stroke::new((1.6 * z).clamp(0.8, 2.0), ink);
    if bus.source { painter.circle_stroke(map(bus.x - 112.0, bus.y), 16.0 * z, stroke); }
    if selected { painter.circle_stroke(map(bus.x, bus.y), (14.0 * z).max(4.0), Stroke::new(1.5, ink)); }
    painter.line_segment([map(bus.x - 80.0, bus.y), map(bus.x + 80.0, bus.y)], Stroke::new((4.0 * z).clamp(1.0, 6.0), ink));
    let detail = diagram_detail(z);
    if detail == DiagramDetail::Overview { return; }
    let card = bus_card_at(bus, origin, z);
    if !card.intersects(painter.clip_rect()) { return; }
    painter.rect_filled(card, 0.0, color("#0A0C10"));
    painter.rect_stroke(card, 0.0, Stroke::new(if selected { 2.0 } else { 1.0 }, if selected { ink } else { color("#334155") }), egui::StrokeKind::Inside);
    let padding = 8.0 * z;
    let mut y = card.top() + 6.0 * z;
    let width = (card.width() - 2.0 * padding).max(0.0);
    let title_size = if detail == DiagramDetail::Compact { (13.0 * z).max(11.0) } else { 13.0 * z };
    let mut line = |text: String, size: f32, color: Color32| {
        let height = size * 1.25;
        let row = Rect::from_min_size(Pos2::new(card.left() + padding, y), Vec2::new(width, height));
        paint_diagram_line(&painter.with_clip_rect(card.shrink(2.0).intersect(painter.clip_rect())), row, &text, size, color);
        y += height;
    };
    line(bus.name.to_uppercase().replace(' ', "_"), title_size, ink);
    if detail == DiagramDetail::Compact {
        let summary = if !bus.arc_failures.is_empty() { "ARC FAILED".to_owned() } else { format!("3P {}", ka(bus.fault_3p_ka)) };
        line(summary, 10.0, if bus.arc_failures.is_empty() { color("#94A3B8") } else { color("#FFB300") });
        return;
    }
    let data = color("#94A3B8");
    let size = 11.0 * z;
    // Selection is conveyed by the border, without inserting a row into the card.
    let vpu = bus.v_pu.map(|v| format!("{v:.3}")).unwrap_or_else(|| "--".into());
    let ang = bus.angle_deg.map(|v| format!("{v:.2}")).unwrap_or_else(|| "--".into());
    line(format!("V: {vpu} PU"), size, data);
    line(format!("ANG: {ang} DEG"), size, data);
    line(format!("I_SC: {}", ka(bus.fault_3p_ka)), size, data);
    line(format!("I_LG: {}", ka(bus.fault_lg_ka)), size, data);
    for arc in &bus.arcs {
        line(format!("{:.1} CAL/CM2", arc.cal_cm2), size, color(sld::energy_color(arc.cal_cm2)));
        line(format!("AFB {:.0} IN  {:.3} S{}", arc.afb_in, arc.time_s, if arc.assumed { " ASSUMED" } else { "" }), size, data);
    }
    if !bus.arc_failures.is_empty() { line("ARC FLASH FAILED".into(), size, color("#FFB300")); }
}

fn bus_hit_rect(bus: &sld::BusGlyph, origin: Pos2, z: f32) -> Rect {
    let center = origin + Vec2::new(bus.x as f32, bus.y as f32) * z;
    let symbol = Rect::from_center_size(center, Vec2::new((160.0 * z).max(10.0), (24.0 * z).max(10.0)));
    if diagram_detail(z) == DiagramDetail::Overview { symbol }
    else { symbol.union(bus_card_at(bus, origin, z)) }
}

fn bus_card_at(bus: &sld::BusGlyph, origin: Pos2, z: f32) -> Rect {
    let top = origin + Vec2::new((bus.x + 96.0) as f32, (bus.y - 16.0) as f32) * z;
    Rect::from_min_size(top, Vec2::new(210.0, bus.card_h as f32) * z)
}

fn draw_tcc(ui: &egui::Ui, rect: Rect, plot: &TccPlot) {
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, color("#0A0C10"));
    let plot_rect = Rect::from_min_size(rect.min + Vec2::new(70.0, 48.0), Vec2::new(760.0, 520.0));
    painter.rect_filled(plot_rect, 0.0, color("#0A0C10"));
    painter.rect_stroke(plot_rect, 0.0, Stroke::new(1.5, color("#334155")), egui::StrokeKind::Inside);
    painter.text(rect.min + Vec2::new(16.0, 8.0), Align2::LEFT_TOP, "TIME-CURRENT CURVE", FontId::monospace(14.0), color("#E2E8F0"));
    painter.text(
        rect.min + Vec2::new(16.0, 28.0),
        Align2::LEFT_TOP,
        format!("Current referred to {:.3} kV", plot.ref_kv),
        FontId::monospace(11.0),
        color("#94A3B8"),
    );
    let x_of = |amp: f64| {
        let t = ((amp.log10() - plot.i_min.log10()) / (plot.i_max.log10() - plot.i_min.log10())) as f32;
        plot_rect.left() + t * plot_rect.width()
    };
    let y_of = |time: f64| {
        let t = ((time.log10() - plot.t_min.log10()) / (plot.t_max.log10() - plot.t_min.log10())) as f32;
        plot_rect.bottom() - t * plot_rect.height()
    };
    for decade in tcc::decades(plot.i_min, plot.i_max) {
        let x = x_of(decade);
        painter.line_segment([Pos2::new(x, plot_rect.top()), Pos2::new(x, plot_rect.bottom())], Stroke::new(1.0, color("#334155")));
    }
    for decade in tcc::decades(plot.t_min, plot.t_max) {
        let y = y_of(decade);
        painter.line_segment([Pos2::new(plot_rect.left(), y), Pos2::new(plot_rect.right(), y)], Stroke::new(1.0, color("#334155")));
    }
    for marker in &plot.markers {
        if marker.amps < plot.i_min || marker.amps > plot.i_max {
            continue;
        }
        let x = x_of(marker.amps);
        painter.line_segment(
            [Pos2::new(x, plot_rect.top()), Pos2::new(x, plot_rect.bottom())],
            Stroke::new(1.0, color(marker.color)),
        );
    }
    for curve in &plot.curves {
        let mut points = Vec::new();
        for (amp, time) in &curve.points {
            if *amp < plot.i_min || *amp > plot.i_max || *time < plot.t_min || *time > plot.t_max {
                if points.len() > 1 {
                    painter.add(egui::Shape::line(points.clone(), Stroke::new(2.2, color(curve.color))));
                }
                points.clear();
                continue;
            }
            points.push(Pos2::new(x_of(*amp), y_of(*time)));
        }
        if points.len() > 1 {
            painter.add(egui::Shape::line(points, Stroke::new(2.2, color(curve.color))));
        }
    }
    let mut y = plot_rect.top();
    for curve in &plot.curves {
        painter.line_segment([Pos2::new(plot_rect.right() + 16.0, y + 8.0), Pos2::new(plot_rect.right() + 36.0, y + 8.0)], Stroke::new(2.4, color(curve.color)));
        painter.text(Pos2::new(plot_rect.right() + 42.0, y), Align2::LEFT_TOP, curve.name.to_uppercase(), FontId::monospace(11.0), color("#E2E8F0"));
        y += 20.0;
    }
}

fn ka(value: Option<f64>) -> String {
    match value {
        Some(v) => format!("{v:.2} kA"),
        None => "—".into(),
    }
}

fn terminal_visuals() -> egui::Visuals {
    let mut visuals = egui::Visuals::dark();
    let canvas = color("#0A0C10");
    let ink = color("#E2E8F0");
    let grid = color("#334155");
    let phosphor = color("#00FF66");
    visuals.panel_fill = canvas;
    visuals.window_fill = canvas;
    visuals.extreme_bg_color = canvas;
    visuals.faint_bg_color = canvas;
    visuals.code_bg_color = canvas;
    visuals.override_text_color = Some(ink);
    visuals.window_stroke = Stroke::new(1.0, grid);
    visuals.widgets.noninteractive.bg_fill = canvas;
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, ink);
    visuals.widgets.inactive.bg_fill = canvas;
    visuals.widgets.inactive.weak_bg_fill = canvas;
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, ink);
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, grid);
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, phosphor);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, phosphor);
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, phosphor);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0, phosphor);
    visuals.selection.bg_fill = Color32::from_rgba_unmultiplied(0, 255, 102, 48);
    visuals.selection.stroke = Stroke::new(1.0, phosphor);
    visuals.warn_fg_color = color("#FFB300");
    visuals.error_fg_color = color("#FFB300");
    visuals.hyperlink_color = phosphor;
    visuals
}

fn color(hex: &str) -> Color32 {
    let hex = hex.trim_start_matches('#');
    let n = u32::from_str_radix(hex, 16).unwrap_or(0x222222);
    Color32::from_rgb((n >> 16) as u8, (n >> 8) as u8, n as u8)
}

fn write_status(path: &std::path::Path, text: &str) -> String {
    match std::fs::write(path, text) {
        Ok(()) => format!("Wrote {}", path.display()),
        Err(err) => err.to_string(),
    }
}

#[cfg(test)]
mod zoom_tests {
    use super::*;

    #[test]
    fn grid_is_viewport_bounded_and_covers_both_sides_of_the_origin() {
        for zoom in [0.05, 0.1, 0.399, 0.4, 0.6, 0.95, 1.0, 4.0, 8.0] {
            for origin in [-1_000_000.0, -333.0, 500.0, 1_000_000.0] {
                let lines = grid_lines(100.0, 1100.0, origin, zoom);
                assert!(lines.len() <= 43 && lines.len() >= 20);
                assert!(lines.first().unwrap().0 < 148.01);
                assert!(lines.last().unwrap().0 > 1051.99);
                assert!(lines.iter().all(|(x, alpha)| *x >= 100.0 && *x <= 1100.0 && (0.0..=1.0).contains(alpha)));
                for pair in lines.windows(2) {
                    assert!((24.0-0.01..=48.0+0.01).contains(&(pair[1].0-pair[0].0)));
                }
            }
        }
        assert!(grid_lines(0.0, 100.0, f32::INFINITY, 1.0).is_empty());
    }

    #[test]
    fn pinch_keeps_the_diagram_point_under_the_pointer() {
        let pan = Vec2::new(-400.0, 137.0);
        let pointer = Vec2::new(522.0, 241.0);
        for new in [0.05, 0.4, 0.95, 2.0, 8.0] {
            let result = zoom_pan(pan, pointer, 1.3, new);
            let before = (pointer - pan) / 1.3;
            let after = (pointer - result) / new;
            assert!((before-after).length() < 0.002);
        }
    }

    #[test]
    fn painted_bus_labels_do_not_overlap_or_escape_cards_at_any_zoom() {
        let mut project = Project::sample();
        project.motors.clear(); // Keep a completed arc row for this geometry-only test.
        let results = study::run(&project, Studies::all()).unwrap();
        let mut bus = sld::diagram(&project, &results).buses[1].clone();
        bus.x = 0.0; bus.y = 30.0;
        bus.name = "A very long equipment name that must be truncated inside the card".into();
        let arc = bus.arcs[0].clone();
        bus.arcs = vec![arc; 4];
        bus.arc_failures = vec!["missing data".into()];
        bus.card_h = 112.0 + 4.0*30.0 + 20.0;
        for z in [0.05, 0.2, 0.399, 0.4, 0.65, 0.949, 0.95, 1.0, 2.0, 8.0] {
            for selected in [false, true] {
                let ctx = egui::Context::default();
                let origin = Pos2::new(150.0, 100.0);
                let viewport = Rect::from_min_size(Pos2::ZERO, Vec2::splat(5000.0));
                let input = egui::RawInput { screen_rect: Some(viewport), ..Default::default() };
                let mut output = ctx.run_ui(input, |ui| draw_bus(ui.painter(), &bus, origin, z, selected));
                // This headless geometry test intentionally does not upload font textures.
                output.textures_delta.clear();
                let mut rows = Vec::new();
                for clipped in &output.shapes {
                    if let egui::epaint::Shape::Text(text) = &clipped.shape {
                        let visible = text.visual_bounding_rect().intersect(clipped.clip_rect);
                        assert!(bus_card_at(&bus, origin, z).contains_rect(visible), "text escapes at zoom {z}");
                        assert!(text.galley.rows.len() <= 1, "text wraps at zoom {z}");
                        if visible.is_positive() { rows.push(visible); }
                    }
                }
                if z < 0.4 { assert!(rows.is_empty()); }
                else {
                    assert!(!rows.is_empty());
                    if z < 0.95 { assert_eq!(rows.len(), 2); }
                    for pair in rows.windows(2) {
                        assert!(pair[0].bottom() <= pair[1].top(), "overlapping labels at zoom {z}: {pair:?}");
                    }
                }
            }
        }
    }
}
