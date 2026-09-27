use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Stroke, Vec2};

use flashmob::model::Project;
use flashmob::sld::{self, Diagram};
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
        ui.horizontal(|ui| {
            ui.label("Zoom");
            ui.add(egui::Slider::new(&mut self.zoom, 0.05..=8.0).logarithmic(true));
        });
        let view = ui.available_rect_before_wrap();
        let (rect, response) = ui.allocate_exact_size(view.size(), Sense::click_and_drag());
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
                let old = self.zoom;
                let new = (old * zoom_delta).clamp(0.05, 8.0);
                let local = pos - rect.min;
                let diagram_pt = (local - self.pan) / old;
                self.pan = local - diagram_pt * new;
                self.zoom = new;
            }
        }
        if response.hovered() {
            self.pan += ui.input(|i| i.smooth_scroll_delta);
        }
        let mut painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, color("#0A0C10"));
        painter.set_clip_rect(rect);
        let origin = rect.min + self.pan;
        let z = self.zoom;
            let map = |x: f64, y: f64| origin + Vec2::new(x as f32, y as f32) * z;
            let grid = Stroke::new(1.0, color("#334155"));
            let mut gx = origin.x;
            while gx < rect.right() {
                painter.line_segment([Pos2::new(gx, rect.top()), Pos2::new(gx, rect.bottom())], grid);
                gx += 40.0 * z;
            }
            let mut gy = origin.y;
            while gy < rect.bottom() {
                painter.line_segment([Pos2::new(rect.left(), gy), Pos2::new(rect.right(), gy)], grid);
                gy += 40.0 * z;
            }
            painter.text(
                origin + Vec2::new(16.0, 8.0),
                Align2::LEFT_TOP,
                "ONE-LINE  IEEE 1584-2018  SYMMETRICAL RMS",
                FontId::monospace(13.0),
                color("#94A3B8"),
            );
            let ink = color("#E2E8F0");
            for branch in &diagram.branches {
                let stroke = Stroke::new(2.0, ink);
                let pts = branch.points();
                for pair in pts.windows(2) {
                    painter.line_segment([map(pair[0].0, pair[0].1), map(pair[1].0, pair[1].1)], stroke);
                }
                let (wx, wy) = branch.winding_at();
                if branch.transformer {
                    painter.circle_stroke(map(wx, wy - 10.0), 12.0 * z, Stroke::new(1.5, ink));
                    painter.circle_stroke(map(wx, wy + 10.0), 12.0 * z, Stroke::new(1.5, ink));
                }
                if branch.protector != "none" {
                    let (dx, dy) = branch.device_at();
                    let mark = map(dx, dy);
                    let w = 14.0 * z;
                    let h = if branch.protector == "switch" { 10.0 * z } else { 14.0 * z };
                    let box_rect = Rect::from_center_size(mark, Vec2::new(w, h));
                    painter.rect_stroke(box_rect, 0.0, Stroke::new(1.4, ink), egui::StrokeKind::Inside);
                }
                painter.text(
                    map(wx + 20.0, wy),
                    Align2::LEFT_CENTER,
                    branch.name.to_uppercase(),
                    FontId::monospace(10.0),
                    color("#94A3B8"),
                );
            }
            if response.clicked() {
                self.selected = None;
                if let Some(pointer) = response.hover_pos() {
                    for bus in &diagram.buses {
                        if bus_card(&diagram, bus, origin, z).contains(pointer) {
                            self.selected = Some(bus.id.clone());
                        }
                    }
                }
            }
            for bus in &diagram.buses {
                draw_bus(&painter, bus, origin, z, self.selected.as_deref() == Some(&bus.id));
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
        ui.label(format!("Prefault {}", fault.prefault));
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
                ui.label(bus.three_phase.as_ref().map(|p| format!("{:.1}", p.x_over_r)).unwrap_or_else(|| "—".into()));
                ui.label(ka(bus.three_phase.as_ref().map(|p| p.iec_peak_ka)));
                ui.end_row();
            }
        });
    }

    fn arc(&self, ui: &mut egui::Ui) {
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
                ui.label(format!("{seconds:.3}"));
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
        let Ok(plot) = tcc::plot(&self.project, Some(&results), Some(self.tcc_kv), &[]) else {
            ui.label("Add a protective device to draw a time-current curve.");
            return;
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
        let Some((name, kv)) = self.project.buses.iter_mut().find(|b| b.id == id).map(|bus| {
            ui.label(RichText::new(&bus.name).strong());
            ui.horizontal(|ui| {
                ui.label("kV");
                ui.add(egui::DragValue::new(&mut bus.kv).speed(0.01).range(0.05..=40.0));
            });
            (bus.name.clone(), bus.kv)
        }) else {
            ui.label("That bus is no longer in the project.");
            return;
        };
        if let Some(results) = &self.results {
            if let Some(fault) = results.fault.as_ref().and_then(|f| f.buses.iter().find(|b| b.id == id)) {
                ui.separator();
                ui.label(RichText::new("Short circuit").strong());
                if let Some(p) = &fault.three_phase {
                    ui.label(format!("3P  {:.2} kA   X/R {:.1}   peak {:.2} kA", p.symmetrical_ka, p.x_over_r, p.iec_peak_ka));
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
                    ui.label(format!("IEEE 1584-2018  {}", row.electrode));
                    if row.assumed {
                        ui.label("Enclosure is assumed from the bus voltage.");
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
        let Ok(plot) = tcc::plot(&self.project, Some(results), Some(self.tcc_kv), &[]) else {
            self.status = "No devices to plot.".into();
            return;
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

fn draw_bus(painter: &egui::Painter, bus: &sld::BusGlyph, origin: Pos2, z: f32, selected: bool) {
    let a = origin + Vec2::new((bus.x - 80.0) as f32, bus.y as f32) * z;
    let b = origin + Vec2::new((bus.x + 80.0) as f32, bus.y as f32) * z;
    let ink = if selected { color("#00FF66") } else { color("#E2E8F0") };
    if bus.source {
        let c = origin + Vec2::new((bus.x - 112.0) as f32, bus.y as f32) * z;
        painter.circle_stroke(c, 16.0 * z, Stroke::new(1.6, ink));
    }
    if selected {
        let c = origin + Vec2::new(bus.x as f32, bus.y as f32) * z;
        painter.circle_stroke(c, 8.0 * z, Stroke::new(2.5, color("#00FF66")));
        painter.circle_stroke(c, 14.0 * z, Stroke::new(1.5, color("#00FF66")));
    }
    painter.line_segment([a, b], Stroke::new(4.0 * z.max(0.8), ink));
    let card = bus_card_at(bus, origin, z);
    painter.rect_stroke(
        card,
        0.0,
        Stroke::new(if selected { 2.5 } else { 1.5 }, if selected { color("#00FF66") } else { color("#334155") }),
        egui::StrokeKind::Inside,
    );
    let mut y = card.top() + 6.0;
    let x = card.left() + 8.0;
    let label = color(if selected { "#00FF66" } else { "#E2E8F0" });
    let data = color("#94A3B8");
    painter.text(Pos2::new(x, y), Align2::LEFT_TOP, bus.name.to_uppercase().replace(' ', "_"), FontId::monospace(13.0), label);
    y += 16.0;
    if selected {
        painter.text(Pos2::new(x, y), Align2::LEFT_TOP, ">> FAULT LOCUS", FontId::monospace(12.0), color("#00FF66"));
        y += 14.0;
    }
    let vpu = bus.v_pu.map(|v| format!("{v:.3}")).unwrap_or_else(|| "--".into());
    let ang = bus.angle_deg.map(|v| format!("{v:.2}")).unwrap_or_else(|| "--".into());
    painter.text(Pos2::new(x, y), Align2::LEFT_TOP, format!("V: {vpu} PU"), FontId::monospace(11.0), data);
    y += 14.0;
    painter.text(Pos2::new(x, y), Align2::LEFT_TOP, format!("ANG: {ang} DEG"), FontId::monospace(11.0), data);
    y += 14.0;
    painter.text(Pos2::new(x, y), Align2::LEFT_TOP, format!("I_SC: {}", ka(bus.fault_3p_ka)), FontId::monospace(11.0), data);
    y += 14.0;
    painter.text(Pos2::new(x, y), Align2::LEFT_TOP, format!("I_LG: {}", ka(bus.fault_lg_ka)), FontId::monospace(11.0), data);
    for arc in &bus.arcs {
        y += 14.0;
        painter.text(
            Pos2::new(x, y),
            Align2::LEFT_TOP,
            format!("{:.1} CAL/CM2", arc.cal_cm2),
            FontId::monospace(11.0),
            color(sld::energy_color(arc.cal_cm2)),
        );
        y += 14.0;
        painter.text(
            Pos2::new(x, y),
            Align2::LEFT_TOP,
            format!("AFB {:.0} IN  {:.3} S{}", arc.afb_in, arc.time_s, if arc.assumed { "  ASSUMED" } else { "" }),
            FontId::monospace(11.0),
            data,
        );
    }
}

fn bus_card(diagram: &Diagram, bus: &sld::BusGlyph, origin: Pos2, z: f32) -> Rect {
    let _ = diagram;
    bus_card_at(bus, origin, z)
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
    let mut decade = 10_f64.powf(plot.i_min.log10().floor());
    while decade <= plot.i_max {
        if decade >= plot.i_min {
            let x = x_of(decade);
            painter.line_segment([Pos2::new(x, plot_rect.top()), Pos2::new(x, plot_rect.bottom())], Stroke::new(1.0, color("#334155")));
        }
        decade *= 10.0;
    }
    decade = 10_f64.powf(plot.t_min.log10().floor());
    while decade <= plot.t_max {
        if decade >= plot.t_min {
            let y = y_of(decade);
            painter.line_segment([Pos2::new(plot_rect.left(), y), Pos2::new(plot_rect.right(), y)], Stroke::new(1.0, color("#334155")));
        }
        decade *= 10.0;
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
