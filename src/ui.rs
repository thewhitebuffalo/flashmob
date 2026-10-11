use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Stroke, Vec2};

use flashmob::library::Library;
use flashmob::model::{BandInterpretation, CurveSpec, Device, IecKind, IeeeKind, PointTail, Project};
use flashmob::sld;
use flashmob::study::{self, InterruptingDuty, Studies, StudyOutput};
use flashmob::tcc::{self, TccPlot};

const STALE_STATUS: &str = "Results stale — model edited; run the study";

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
    /// A failed open/export. Shown prominently until dismissed or replaced by a
    /// successful open; running a study never overwrites it.
    error: Option<String>,
    show_warnings: bool,
    zoom: f32,
    /// Top-left of the diagram, in points inside the fixed one-line panel.
    pan: Vec2,
    fit_view: bool,
    tcc_kv: f64,
    /// Derived from `project` and `results`; cleared whenever either changes.
    diagram: Option<sld::Diagram>,
    /// TCC for the reference voltage it was drawn at; cleared with the diagram.
    tcc_plot: Option<(f64, Result<TccPlot, String>)>,
    /// Device selected on the Devices tab. Independent of the one-line bus selection.
    selected_device: Option<String>,
    /// Library picker. The file is not stored on the project.
    library_open: bool,
    library: Option<Library>,
    library_path: String,
    library_search: String,
    /// Device ids for `library_search`. Rebuilt when the search text or the open library changes.
    library_hits: Vec<u32>,
    library_hits_for: String,
    library_selected: Option<u32>,
    library_error: String,
    library_form: LibraryForm,
}

fn read_project_file(path: &std::path::Path) -> Result<Project, String> {
    std::fs::read_to_string(path)
        .map_err(|err| err.to_string())
        .and_then(|text| flashmob::exec::load_project(&text))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Sld,
    Flow,
    Fault,
    Arc,
    Tcc,
    Devices,
}

impl App {
    fn new(path: Option<std::path::PathBuf>) -> Self {
        let mut app = Self {
            project: Project::sample(),
            results: None,
            tab: Tab::Sld,
            selected: None,
            status: String::new(),
            error: None,
            show_warnings: false,
            zoom: 1.0,
            pan: Vec2::ZERO,
            fit_view: true,
            tcc_kv: 0.48,
            diagram: None,
            tcc_plot: None,
            selected_device: None,
            library_open: false,
            library: None,
            library_path: String::new(),
            library_search: String::new(),
            library_hits: Vec::new(),
            library_hits_for: "\u{1}".into(),
            library_selected: None,
            library_error: String::new(),
            library_form: LibraryForm::default(),
        };
        match path {
            Some(path) => match read_project_file(&path) {
                Ok(project) => {
                    app.project = project;
                    app.run_study();
                    app.status = format!("Opened {}. {}", path.display(), app.status);
                }
                Err(err) => {
                    // Do not substitute the sample for the file the user asked for.
                    app.project = Project::default();
                    app.error = Some(format!("Could not open {}: {err}", path.display()));
                    app.status = "No project loaded. Use Open to retry or Sample for the built-in example.".into();
                }
            },
            None => {
                app.run_study();
                app.status = format!("Built-in sample plant loaded. {}", app.status);
            }
        }
        app
    }

    /// Drop results and every view derived from them (call after any model edit).
    fn invalidate(&mut self) {
        self.results = None;
        self.diagram = None;
        self.tcc_plot = None;
    }

    fn run_study(&mut self) {
        self.invalidate();
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
                    self.selected_device = None;
                    self.fit_view = true;
                    self.error = None;
                    self.run_study();
                    self.status = format!("Built-in sample plant loaded. {}", self.status);
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
                if ui.button("Export Package").clicked() {
                    self.export_package();
                }
                if ui.button("Library").clicked() {
                    self.library_open = true;
                    self.autoload_library();
                }
            });
            ui.horizontal(|ui| {
                for (tab, label) in [
                    (Tab::Sld, "One-line"),
                    (Tab::Flow, "Load flow"),
                    (Tab::Fault, "Short circuit"),
                    (Tab::Arc, "Arc flash"),
                    (Tab::Tcc, "TCC"),
                    (Tab::Devices, "Devices"),
                ] {
                    ui.selectable_value(&mut self.tab, tab, label);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new("IEEE 1584-2018").color(color("#00FF66")));
                });
            });
        });
        egui::Panel::bottom("status").show(ui, |ui| {
            let mut dismiss = false;
            if let Some(error) = &self.error {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("ERROR: {error}")).strong().size(14.0).color(color("#FF5555")));
                    dismiss = ui.button("Dismiss").clicked();
                });
            }
            if dismiss { self.error = None; }
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
        self.library_window(ui.ctx());
        egui::Panel::right("inspector").default_size(300.0).show(ui, |ui| {
            self.inspector(ui);
        });
        egui::CentralPanel::default().show(ui, |ui| match self.tab {
            Tab::Sld => self.sld(ui),
            Tab::Flow => self.flow(ui),
            Tab::Fault => self.fault(ui),
            Tab::Arc => self.arc(ui),
            Tab::Tcc => self.tcc(ui),
            Tab::Devices => self.devices(ui),
        });
    }
}

impl App {
    fn sld(&mut self, ui: &mut egui::Ui) {
        if self.diagram.is_none() {
            if let Some(results) = &self.results {
                self.diagram = Some(sld::diagram(&self.project, results));
            }
        }
        // Taken for this frame and put back at the end; rebuilt only after results change.
        let Some(diagram) = self.diagram.take() else {
            ui.label("Run the study to draw the one-line.");
            return;
        };
        self.draw_sld(ui, &diagram);
        self.diagram = Some(diagram);
    }

    fn draw_sld(&mut self, ui: &mut egui::Ui, diagram: &sld::Diagram) {
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
        let warn = color("#FFB300");
        for branch in &diagram.branches {
            let stroke = Stroke::new((2.0 * z).clamp(0.8, 2.0), ink);
            let pts: Vec<Pos2> = branch.points().iter().map(|&(x, y)| map(x, y)).collect();
            if branch.closed {
                for pair in pts.windows(2) {
                    painter.line_segment([pair[0], pair[1]], stroke);
                }
            } else {
                // Normally open: dashed, dimmed.
                let dashed = Stroke::new(stroke.width, ink.gamma_multiply(0.6));
                painter.extend(egui::Shape::dashed_line(&pts, dashed, (8.0 * z).max(2.0), (6.0 * z).max(2.0)));
            }
            let (wx, wy) = branch.winding_at();
            if branch.transformer {
                painter.circle_stroke(map(wx, wy - 10.0), 12.0 * z, stroke);
                painter.circle_stroke(map(wx, wy + 10.0), 12.0 * z, stroke);
            }
            let (dx, dy) = branch.device_at();
            match branch.protector.as_str() {
                "breaker" => {
                    let box_rect = Rect::from_center_size(map(dx, dy), Vec2::splat(14.0) * z);
                    painter.rect_filled(box_rect, 0.0, color("#0A0C10"));
                    painter.rect_stroke(box_rect, 0.0, stroke, egui::StrokeKind::Inside);
                }
                "fuse" => {
                    let body = Rect::from_center_size(map(dx, dy), Vec2::new(8.0, 18.0) * z);
                    painter.rect_filled(body, 0.0, color("#0A0C10"));
                    painter.rect_stroke(body, 0.0, stroke, egui::StrokeKind::Inside);
                    painter.line_segment([map(dx, dy - 9.0), map(dx, dy + 9.0)], stroke);
                }
                "switch" => draw_switch(&painter, map, dx, dy, branch.closed, z),
                _ => {}
            }
            if !branch.closed {
                let (sx, sy) = branch.switch_at();
                if branch.protector != "switch" { draw_switch(&painter, map, sx, sy, false, z); }
                if diagram_detail(z) != DiagramDetail::Overview {
                    let label = Rect::from_min_size(map(sx + 14.0, sy - 6.0), Vec2::new(150.0, 14.0) * z);
                    let text = match &branch.switch_name { Some(name) => format!("N.O. {}", name.to_uppercase()), None => "N.O.".into() };
                    paint_diagram_line(&painter, label, &text, 10.0 * z, warn);
                }
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
                for worst in &bus.worst { ui.label(format!("{}: {}", worst.equipment_id, worst.label())); }
                for failure in &bus.arc_failures { ui.label(format!("Arc flash FAILED: {failure}")); }
                if bus.source_out_of_service { ui.label("Source out of service in the normal state."); }
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
        let Some(results) = &self.results else {
            ui.label("No short-circuit result.");
            return;
        };
        let Some(fault) = results.fault.as_ref() else {
            ui.label("No short-circuit result.");
            return;
        };
        egui::ScrollArea::vertical().id_salt("fault-scroll").show(ui, |ui| {
            ui.label(format!("Prefault requested: {}; used: {}", fault.prefault_requested, fault.prefault));
            if !fault.valid {
                ui.label(fault.error.as_deref().unwrap_or("Invalid fault result"));
                return;
            }
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
            if !results.interrupting.is_empty() {
                ui.add_space(8.0);
                ui.label(RichText::new("Interrupting duty").strong());
                duty_grid(ui, &results.interrupting);
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
            // Wide range; existing values are never rewritten by the widget.
            ui.add(egui::DragValue::new(&mut self.tcc_kv).speed(0.01).range(0.001..=1000.0).clamp_existing_to_range(false).suffix(" kV"));
        });
        let Some(results) = &self.results else {
            ui.label("Run the study before drawing curves.");
            return;
        };
        if self.tcc_plot.as_ref().map_or(true, |(kv, _)| *kv != self.tcc_kv) {
            self.tcc_plot = Some((self.tcc_kv, tcc::plot(&self.project, Some(results), Some(self.tcc_kv), &[])));
        }
        match &self.tcc_plot {
            Some((_, Ok(plot))) => {
                egui::ScrollArea::both().show(ui, |ui| {
                    let (rect, _) = ui.allocate_exact_size(Vec2::new(980.0, 680.0), Sense::hover());
                    draw_tcc(ui, rect, plot);
                });
            }
            Some((_, Err(err))) => { ui.label(err.as_str()); }
            None => {}
        }
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
                // A wide drag range that never rewrites an existing value; validate() owns limits.
                edited |= ui.add(egui::DragValue::new(&mut bus.kv).speed(0.01).range(0.001..=1000.0).clamp_existing_to_range(false)).changed();
            });
            (bus.name.clone(), bus.kv)
        }) else {
            ui.label("That bus is no longer in the project.");
            return;
        };
        if edited { self.invalidate(); self.status = STALE_STATUS.into(); }
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
            let duties: Vec<&InterruptingDuty> = results.interrupting.iter().filter(|row| row.bus_id == id).collect();
            if !duties.is_empty() {
                ui.separator();
                ui.label(RichText::new("Interrupting").strong());
                for row in duties {
                    duty_label(ui, row);
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
            if !results.operating_cases.is_empty() {
                if let Some(envelope) = &results.worst_case {
                    for item in envelope.arc_equipment.iter().filter(|e| e.bus_id == id) {
                        ui.separator();
                        ui.label(RichText::new(format!("Worst case · {}", item.equipment_id)).strong());
                        match &item.incident_energy_cal_cm2 {
                            Some(max) => { ui.label(RichText::new(format!("{:.2} cal/cm²  (case {})", max.value, max.case_id)).color(color(sld::energy_color(max.value)))); }
                            None => { ui.label("Incident energy unavailable in every case."); }
                        }
                        if let Some(max) = &item.boundary_mm {
                            ui.label(format!("AFB {:.0} in  (case {})", max.value / 25.4, max.case_id));
                        }
                        if !item.complete {
                            ui.label(RichText::new(format!("INCOMPLETE — failed cases: {}", item.failed_case_ids.join(", "))).color(color("#FFB300")));
                        }
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
            match read_project_file(&path) {
                Ok(project) => {
                    self.project = project;
                    self.selected = None;
                    self.selected_device = None;
                    self.fit_view = true;
                    self.error = None;
                    self.run_study();
                    self.status = format!("Opened {}. {}", path.display(), self.status);
                }
                // The current project stays loaded; the failure stays visible.
                Err(err) => self.error = Some(format!("Could not open {}: {err}", path.display())),
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
            let svg = match &self.diagram {
                Some(diagram) => sld::to_svg(diagram),
                None => sld::to_svg(&sld::diagram(&self.project, results)),
            };
            match write_status(&path, &svg) {
                Ok(message) => self.status = message,
                Err(err) => self.error = Some(err),
            }
        }
    }

    fn export_package(&mut self) {
        let errors = self.project.validate();
        if !errors.is_empty() {
            self.error = Some(format!("Package export refused, project is invalid: {}", errors.join("; ")));
            return;
        }
        if let Some(path) = rfd::FileDialog::new().pick_folder() {
            // Never replace existing files silently; ask for an empty folder instead.
            match flashmob::package::write_dir(&self.project, &path, flashmob::package::WriteOptions::default()) {
                Ok(dir) => self.status = format!("Package export written to {}", dir.display()),
                Err(err) => self.error = Some(format!("Package export failed: {err}")),
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
            let csv_path = path.with_extension("csv");
            let svg = std::fs::write(&path, tcc::to_svg(&plot)).map_err(|err| format!("could not write {}: {err}", path.display()));
            let csv = std::fs::write(&csv_path, tcc::to_csv(&plot)).map_err(|err| format!("could not write {}: {err}", csv_path.display()));
            // Report both outcomes; one success never hides the other failure.
            match (svg, csv) {
                (Ok(()), Ok(())) => self.status = format!("Wrote {} and {}", path.display(), csv_path.display()),
                (Ok(()), Err(err)) => { self.status = format!("Wrote {}", path.display()); self.error = Some(err); }
                (Err(err), Ok(())) => { self.status = format!("Wrote {}", csv_path.display()); self.error = Some(err); }
                (Err(a), Err(b)) => self.error = Some(format!("{a}; {b}")),
            }
        }
    }

    fn mark_stale(&mut self) {
        self.invalidate();
        self.status = STALE_STATUS.into();
    }

    fn devices(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().id_salt("devices-tab").auto_shrink([false, false]).show(ui, |ui| {
            self.device_list(ui);
            ui.separator();
            self.device_editor(ui);
        });
    }

    fn device_list(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("DEVICES").strong().color(color("#E2E8F0")));
        if self.project.devices.is_empty() {
            ui.label("No devices.");
            return;
        }
        let mut choose = None;
        egui::Grid::new("device-list").striped(true).show(ui, |ui| {
            for heading in ["Id", "Name", "Bus", "Curve"] {
                ui.label(RichText::new(heading).strong());
            }
            ui.end_row();
            for device in &self.project.devices {
                let selected = self.selected_device.as_deref() == Some(device.id.as_str());
                let ink = if selected { color("#00FF66") } else { color("#E2E8F0") };
                let mut clicked = false;
                clicked |= ui.selectable_label(selected, RichText::new(&device.id).color(ink)).clicked();
                clicked |= ui.selectable_label(selected, RichText::new(&device.name).color(ink)).clicked();
                clicked |= ui.selectable_label(selected, RichText::new(&device.bus).color(ink)).clicked();
                clicked |= ui.selectable_label(selected, RichText::new(flashmob::curves::curve_label(&device.curve)).color(ink)).clicked();
                if clicked {
                    choose = Some(device.id.clone());
                }
                ui.end_row();
            }
        });
        if let Some(id) = choose {
            self.selected_device = Some(id);
        }
    }

    fn device_editor(&mut self, ui: &mut egui::Ui) {
        let Some(id) = self.selected_device.clone() else {
            ui.label("Select a device.");
            return;
        };
        if !self.project.devices.iter().any(|device| device.id == id) {
            self.selected_device = None;
            ui.label("That device is no longer in the project.");
            return;
        }
        ui.label(RichText::new(format!("ID  {id}")).strong().color(color("#00FF66")));
        let mut edited = false;
        let mut remove = false;
        let mut refusal = None;
        if let Some(device) = self.project.devices.iter_mut().find(|device| device.id == id) {
            edited |= labeled_text(ui, "Name", &mut device.name);
            edited |= labeled_text(ui, "Bus", &mut device.bus);
            ui.label(RichText::new("Bus is checked on Run.").color(color("#94A3B8")));
            edited |= edit_breaker_time(ui, device, &mut refusal);
            edited |= edit_clearing_flags(ui, device, &mut refusal);
            edited |= labeled_text(ui, "Basis", &mut device.basis);
            edited |= edit_interrupting(ui, device);
            ui.separator();
            let curve_id = device.id.clone();
            edited |= edit_curve(ui, &curve_id, &mut device.curve);
            if let Some(link) = &device.library {
                ui.separator();
                ui.label(RichText::new("Library").strong());
                ui.label(format!("Manufacturer  {}", link.manufacturer));
                ui.label(format!("Style  {}", link.style));
                ui.label(format!("Category  {}", link.category));
                if link.unsupported_elements.is_empty() {
                    ui.label(RichText::new("unsupported_elements  none").color(color("#94A3B8")));
                } else {
                    ui.label("unsupported_elements");
                    for item in &link.unsupported_elements {
                        ui.label(RichText::new(item).color(color("#94A3B8")));
                    }
                }
            }
            ui.separator();
            if ui.button("Remove").clicked() {
                remove = true;
            }
        }
        if remove {
            if let Some(index) = self.project.devices.iter().position(|device| device.id == id) {
                self.project.devices.remove(index);
            }
            self.selected_device = None;
            edited = true;
        }
        if edited {
            self.mark_stale();
            if let Some(message) = refusal {
                self.status = format!("{message} {STALE_STATUS}");
            }
        } else if let Some(message) = refusal {
            self.status = message;
        }
    }

    fn library_window(&mut self, ctx: &egui::Context) {
        if !self.library_open {
            return;
        }
        let mut open = true;
        egui::Window::new("LIBRARY")
            .collapsible(false)
            .resizable(true)
            .default_width(760.0)
            .open(&mut open)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().id_salt("library-scroll").max_height(640.0).show(ui, |ui| {
                    self.library_contents(ui);
                });
            });
        self.library_open = open;
    }

    fn library_contents(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui.button("Open library").clicked() {
                self.open_library();
            }
            if !self.library_path.is_empty() {
                ui.label(RichText::new(&self.library_path).color(color("#94A3B8")));
            }
        });
        ui.label(RichText::new("The library file is not stored in the project.").color(color("#94A3B8")));
        if !self.library_error.is_empty() {
            ui.label(RichText::new(&self.library_error).color(color("#FF5555")));
        }
        let device_count = self.library.as_ref().map(|library| library.devices.len());
        if let Some(count) = device_count {
            ui.label(RichText::new(format!("{count} devices")).color(color("#94A3B8")));
            ui.horizontal(|ui| {
                ui.label("Search");
                ui.text_edit_singleline(&mut self.library_search);
            });
            self.library_matches(ui);
        } else {
            ui.label("No library open.");
        }
        ui.separator();
        self.library_form(ui);
    }

    fn autoload_library(&mut self) {
        if self.library.is_some() {
            return;
        }
        match flashmob::library::open_default_library() {
            Ok((library, name)) => self.store_library(library, name),
            Err(err) => self.library_error = err,
        }
    }

    fn open_library(&mut self) {
        let Some(path) = rfd::FileDialog::new().add_filter("Device library", &["json"]).pick_file() else { return };
        self.load_library_file(&path);
    }

    fn load_library_file(&mut self, path: &std::path::Path) {
        match std::fs::read_to_string(path).map_err(|err| err.to_string()).and_then(|text| Library::from_json(&text)) {
            Ok(library) => self.store_library(library, path.display().to_string()),
            // A bad file stays in this window. The open project is left as it is.
            Err(err) => self.library_error = format!("Could not open {}: {err}", path.display()),
        }
    }

    fn store_library(&mut self, library: Library, name: String) {
        let count = library.devices.len();
        self.library_path = name;
        self.library = Some(library);
        self.library_selected = None;
        self.library_hits.clear();
        self.library_hits_for = "\u{1}".into();
        self.library_error.clear();
        self.status = format!("Opened library, {count} devices. {}", self.library_path);
    }

    fn library_matches(&mut self, ui: &mut egui::Ui) {
        self.refresh_library_hits();
        let current = self.library_selected;
        let choose = self.draw_library_matches(ui, current);
        if let Some(id) = choose {
            self.library_selected = Some(id);
        }
    }

    fn refresh_library_hits(&mut self) {
        if self.library.is_none() {
            self.library_hits.clear();
            self.library_hits_for.clear();
            return;
        }
        if self.library_hits_for == self.library_search {
            return;
        }
        let terms: Vec<String> = self.library_search.split_whitespace().map(str::to_string).collect();
        self.library_hits = self.library.as_ref().map(|library| {
            library.search(&terms).into_iter().map(|device| device.id).collect()
        }).unwrap_or_default();
        self.library_hits_for = self.library_search.clone();
    }

    fn draw_library_matches(&self, ui: &mut egui::Ui, current: Option<u32>) -> Option<u32> {
        let Some(library) = self.library.as_ref() else { return None };
        ui.label(RichText::new(format!("{} matches", self.library_hits.len())).color(color("#94A3B8")));
        if self.library_hits.len() > 40 {
            ui.label(RichText::new("Showing 40.").color(color("#94A3B8")));
        }
        let mut choose = None;
        egui::Grid::new("library-matches").striped(true).show(ui, |ui| {
            for heading in ["Id", "Manufacturer", "Style", "Category"] {
                ui.label(RichText::new(heading).strong());
            }
            ui.end_row();
            for id in self.library_hits.iter().take(40) {
                let Some(device) = library.devices.iter().find(|device| device.id == *id) else { continue };
                let selected = current == Some(device.id);
                let ink = if selected { color("#00FF66") } else { color("#E2E8F0") };
                let mut clicked = false;
                clicked |= ui.selectable_label(selected, RichText::new(device.id.to_string()).color(ink)).clicked();
                clicked |= ui.selectable_label(selected, RichText::new(&device.manufacturer).color(ink)).clicked();
                clicked |= ui.selectable_label(selected, RichText::new(&device.style).color(ink)).clicked();
                clicked |= ui.selectable_label(selected, RichText::new(&device.category).color(ink)).clicked();
                if clicked {
                    choose = Some(device.id);
                }
                ui.end_row();
            }
        });
        if let Some(id) = choose.or(current) {
            if let Some(device) = library.devices.iter().find(|device| device.id == id) {
                show_library_device(ui, device);
            }
        }
        choose
    }

    fn library_form(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("Apply").strong());
        ui.label(RichText::new("Project id and bus are required. Every other field is optional.").color(color("#94A3B8")));
        match self.library_selected {
            Some(id) => ui.label(RichText::new(format!("Library device {id}")).color(color("#00FF66"))),
            None => ui.label(RichText::new("No library device selected.").color(color("#94A3B8"))),
        };
        let apply;
        {
            let form = &mut self.library_form;
            labeled_text(ui, "Project id", &mut form.id);
            labeled_text(ui, "Bus", &mut form.bus);
            labeled_text(ui, "Name", &mut form.name);
            labeled_text(ui, "rating_a", &mut form.rating_a);
            labeled_text(ui, "sensor_a", &mut form.sensor_a);
            labeled_text(ui, "current_base_a", &mut form.current_base_a);
            labeled_text(ui, "voltage_v", &mut form.voltage_v);
            labeled_text(ui, "function_name", &mut form.function_name);
            labeled_text(ui, "dial", &mut form.dial);
            labeled_text(ui, "ct_ratio", &mut form.ct_ratio);
            labeled_text(ui, "tap_a", &mut form.tap_a);
            labeled_pair(ui, "lt_pickup", &mut form.lt_pickup_function, &mut form.lt_pickup_label);
            labeled_pair(ui, "lt_delay", &mut form.lt_delay_function, &mut form.lt_delay_label);
            labeled_text(ui, "lt_delay_multiple", &mut form.lt_delay_multiple);
            labeled_pair(ui, "st_pickup", &mut form.st_pickup_function, &mut form.st_pickup_label);
            labeled_pair(ui, "st_delay", &mut form.st_delay_function, &mut form.st_delay_label);
            labeled_pair(ui, "inst", &mut form.inst_function, &mut form.inst_label);
            labeled_text(ui, "inst_s", &mut form.inst_s);
            labeled_text(ui, "pickup_base", &mut form.pickup_base);
            ui.label(RichText::new("pickup_base is sensor, long_time, or empty.").color(color("#94A3B8")));
            ui.checkbox(&mut form.fuse_total_clearing, "Fuse total clearing");
            apply = ui.button("Apply").clicked();
        }
        if apply {
            self.apply_selected_library();
        }
    }

    fn library_request(&self, device_id: u32) -> Result<flashmob::library::ApplyDevice, String> {
        let id = self.library_form.id.trim();
        let bus = self.library_form.bus.trim();
        if id.is_empty() || bus.is_empty() {
            return Err("Project device id and bus are required.".into());
        }
        if self.library_path.is_empty() {
            return Err("Open a library first.".into());
        }
        let pickup_base = match self.library_form.pickup_base.trim().to_ascii_lowercase().as_str() {
            "" => None,
            "sensor" => Some("sensor".to_string()),
            "long_time" => Some("long_time".to_string()),
            other => return Err(format!("pickup_base must be sensor, long_time, or empty, not {other}.")),
        };
        let text = |value: &str| {
            let trimmed = value.trim();
            if trimmed.is_empty() { None } else { Some(trimmed.to_string()) }
        };
        let number = |value: &str, name: &str| -> Result<Option<f64>, String> {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                return Ok(None);
            }
            let parsed: f64 = trimmed.parse().map_err(|_| format!("{name} is not a number."))?;
            if !parsed.is_finite() {
                return Err(format!("{name} is not finite."));
            }
            Ok(Some(parsed))
        };
        let lt_pickup_function = text(&self.library_form.lt_pickup_function);
        let lt_pickup_label = text(&self.library_form.lt_pickup_label);
        let lt_delay_function = text(&self.library_form.lt_delay_function);
        let lt_delay_label = text(&self.library_form.lt_delay_label);
        let st_pickup_function = text(&self.library_form.st_pickup_function);
        let st_pickup_label = text(&self.library_form.st_pickup_label);
        let st_delay_function = text(&self.library_form.st_delay_function);
        let st_delay_label = text(&self.library_form.st_delay_label);
        let inst_function = text(&self.library_form.inst_function);
        let inst_label = text(&self.library_form.inst_label);
        Ok(flashmob::library::ApplyDevice {
            device_id,
            id: id.to_string(),
            name: self.library_form.name.trim().to_string(),
            bus: bus.to_string(),
            protected_branch: None,
            rating_a: number(&self.library_form.rating_a, "rating_a")?,
            current_base_a: number(&self.library_form.current_base_a, "current_base_a")?,
            voltage_v: number(&self.library_form.voltage_v, "voltage_v")?,
            function_name: text(&self.library_form.function_name),
            library_name: self.library_path.clone(),
            fuse_total_clearing: self.library_form.fuse_total_clearing,
            dial: number(&self.library_form.dial, "dial")?,
            ct_ratio: number(&self.library_form.ct_ratio, "ct_ratio")?,
            tap_a: number(&self.library_form.tap_a, "tap_a")?,
            sensor_a: number(&self.library_form.sensor_a, "sensor_a")?,
            lt_pickup: flashmob::library_apply::setting_ref(&lt_pickup_function, &lt_pickup_label, "lt_pickup")?,
            lt_delay: flashmob::library_apply::setting_ref(&lt_delay_function, &lt_delay_label, "lt_delay")?,
            lt_delay_multiple: number(&self.library_form.lt_delay_multiple, "lt_delay_multiple")?,
            st_pickup: flashmob::library_apply::setting_ref(&st_pickup_function, &st_pickup_label, "st_pickup")?,
            st_delay: flashmob::library_apply::setting_ref(&st_delay_function, &st_delay_label, "st_delay")?,
            inst: flashmob::library_apply::setting_ref(&inst_function, &inst_label, "inst")?,
            inst_s: number(&self.library_form.inst_s, "inst_s")?,
            pickup_base,
        })
    }

    fn apply_selected_library(&mut self) {
        let Some(device_id) = self.library_selected else {
            self.library_error = "Select a library device.".into();
            return;
        };
        if self.library.is_none() {
            self.library_error = "Open a library first.".into();
            return;
        }
        let request = match self.library_request(device_id) {
            Ok(request) => request,
            Err(err) => {
                self.library_error = err;
                return;
            }
        };
        let applied = self.library.as_ref().map(|library| flashmob::library::apply_device(library, &request));
        let Some(applied) = applied else {
            self.library_error = "Open a library first.".into();
            return;
        };
        let device = match applied {
            Ok(device) => device,
            Err(err) => {
                self.library_error = err;
                return;
            }
        };
        if self.project.devices.iter().any(|existing| existing.id == device.id) {
            self.library_error = format!("Device {} is already in the project.", device.id);
            return;
        }
        self.selected_device = Some(device.id.clone());
        self.project.devices.push(device);
        self.library_error.clear();
        self.mark_stale();
    }
}

#[derive(Default)]
struct LibraryForm {
    id: String,
    bus: String,
    name: String,
    rating_a: String,
    sensor_a: String,
    current_base_a: String,
    voltage_v: String,
    function_name: String,
    dial: String,
    ct_ratio: String,
    tap_a: String,
    lt_pickup_function: String,
    lt_pickup_label: String,
    lt_delay_function: String,
    lt_delay_label: String,
    lt_delay_multiple: String,
    st_pickup_function: String,
    st_pickup_label: String,
    st_delay_function: String,
    st_delay_label: String,
    inst_function: String,
    inst_label: String,
    inst_s: String,
    pickup_base: String,
    fuse_total_clearing: bool,
}

fn labeled_text(ui: &mut egui::Ui, label: &str, text: &mut String) -> bool {
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).color(color("#94A3B8")));
        ui.add(egui::TextEdit::singleline(text).desired_width(280.0)).changed()
    }).inner
}

fn labeled_pair(ui: &mut egui::Ui, label: &str, function: &mut String, setting: &mut String) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).color(color("#94A3B8")));
        ui.add(egui::TextEdit::singleline(function).desired_width(160.0).hint_text("function"));
        ui.add(egui::TextEdit::singleline(setting).desired_width(140.0).hint_text("label"));
    });
}

fn drag_value(ui: &mut egui::Ui, label: &str, value: &mut f64, speed: f64, range: std::ops::RangeInclusive<f64>, suffix: &'static str) -> bool {
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).color(color("#94A3B8")));
        let drag = egui::DragValue::new(value).speed(speed).range(range).clamp_existing_to_range(false);
        let drag = if suffix.is_empty() { drag } else { drag.suffix(suffix) };
        ui.add(drag).changed()
    }).inner
}

/// Empty text clears the value. A half-typed number is kept only while the field has focus.
fn optional_f64(ui: &mut egui::Ui, salt: impl egui::AsId, label: &str, value: &mut Option<f64>) -> bool {
    let id = egui::Id::new(salt);
    let ctx = ui.ctx().clone();
    let mut text = ctx.data(|data| data.get_temp::<String>(id)).unwrap_or_else(|| format_num_opt(*value));
    let response = ui.horizontal(|ui| {
        ui.label(RichText::new(label).color(color("#94A3B8")));
        ui.add(egui::TextEdit::singleline(&mut text).desired_width(140.0).hint_text("none"))
    }).inner;
    let mut edited = false;
    if response.changed() {
        if text.trim().is_empty() {
            if value.is_some() {
                *value = None;
                edited = true;
            }
        } else if let Some(parsed) = parse_f64(text.trim()) {
            if *value != Some(parsed) {
                *value = Some(parsed);
                edited = true;
            }
        }
    }
    if response.has_focus() {
        ctx.data_mut(|data| data.insert_temp(id, text));
    } else {
        ctx.data_mut(|data| { data.remove_temp::<String>(id); });
    }
    edited
}

fn edit_breaker_time(ui: &mut egui::Ui, device: &mut Device, refusal: &mut Option<String>) -> bool {
    let blocked = device.fuse_total_clearing || device.curve_is_total_clearing;
    let mut time = device.breaker_interrupting_s;
    let mut dragged = false;
    let mut clear = false;
    let mut set = false;
    ui.horizontal(|ui| {
        ui.label(RichText::new("Breaker interrupting").color(color("#94A3B8")));
        if blocked || time.is_none() {
            let shown = time.map(|value| format!("{value} s")).unwrap_or_else(|| "none".into());
            ui.label(RichText::new(shown).color(color("#94A3B8")));
        } else if let Some(value) = time.as_mut() {
            dragged = ui.add(
                egui::DragValue::new(value).speed(0.001).range(0.0..=1.0e6).clamp_existing_to_range(false).suffix(" s"),
            ).changed();
        }
        if time.is_some() {
            clear = ui.button("Clear").clicked();
        } else {
            set = ui.button("Set").clicked();
        }
    });
    if clear {
        device.breaker_interrupting_s = None;
        return true;
    }
    if set {
        if blocked {
            *refusal = Some("Clear the total-clearing flag before entering a breaker interrupting time.".into());
            return false;
        }
        // Seed for the drag. This is not an interrupting rating.
        device.breaker_interrupting_s = Some(0.05);
        return true;
    }
    if !blocked && dragged {
        device.breaker_interrupting_s = time;
        return true;
    }
    false
}

fn edit_clearing_flags(ui: &mut egui::Ui, device: &mut Device, refusal: &mut Option<String>) -> bool {
    let mut edited = false;
    ui.horizontal(|ui| {
        edited |= toggle_clearing_flag(ui, device, true, "Fuse total clearing", refusal);
        edited |= toggle_clearing_flag(ui, device, false, "Curve is total clearing", refusal);
    });
    ui.label(RichText::new("A total-clearing flag cannot be combined with a breaker interrupting time.").color(color("#94A3B8")));
    edited
}

/// A flag is refused while a breaker time is set. Turning a flag off leaves that time alone.
fn toggle_clearing_flag(ui: &mut egui::Ui, device: &mut Device, fuse: bool, label: &str, refusal: &mut Option<String>) -> bool {
    let mut enabled = if fuse { device.fuse_total_clearing } else { device.curve_is_total_clearing };
    if !ui.checkbox(&mut enabled, label).changed() {
        return false;
    }
    if enabled && device.breaker_interrupting_s.is_some() {
        *refusal = Some("Clear the breaker interrupting time before setting a total-clearing flag.".into());
        return false;
    }
    if fuse {
        device.fuse_total_clearing = enabled;
    } else {
        device.curve_is_total_clearing = enabled;
    }
    if enabled {
        device.breaker_interrupting_s = None;
    }
    true
}

fn edit_interrupting(ui: &mut egui::Ui, device: &mut Device) -> bool {
    let Some((symmetrical_ka, test_xr, voltage_v)) = device.interrupting.as_ref().map(|rating| {
        (rating.symmetrical_ka, rating.test_xr, rating.voltage_v)
    }) else {
        return false;
    };
    ui.separator();
    ui.label(RichText::new("Interrupting rating").strong());
    ui.label(format!("Symmetrical {} kA", format_num(symmetrical_ka)));
    ui.label(format!("Test X/R {}", format_num(test_xr)));
    ui.label(format!("Voltage {} V", format_num(voltage_v)));
    // Shown only when a rating is already on the device. The editor does not invent one.
    if ui.button("Clear interrupting rating").clicked() {
        device.interrupting = None;
        return true;
    }
    false
}

fn edit_curve(ui: &mut egui::Ui, device_id: &str, curve: &mut CurveSpec) -> bool {
    let mut edited = false;
    match curve {
        CurveSpec::Definite { pickup_a, time_s } => {
            ui.label(RichText::new("DEFINITE").strong());
            edited |= drag_value(ui, "Pickup", pickup_a, 10.0, 0.0..=1.0e9, " A");
            edited |= drag_value(ui, "Time", time_s, 0.001, 0.0..=1.0e6, " s");
        }
        CurveSpec::Ieee { kind, pickup_a, td, inst_a, inst_s } => {
            ui.label(RichText::new(format!("IEEE {}", ieee_name(*kind))).strong());
            edited |= drag_value(ui, "Pickup", pickup_a, 10.0, 0.0..=1.0e9, " A");
            edited |= drag_value(ui, "Time dial", td, 0.01, 0.0..=1.0e4, "");
            ui.label(RichText::new("Empty optional fields are cleared.").color(color("#94A3B8")));
            edited |= optional_f64(ui, (device_id, "ieee-inst-a"), "Inst", inst_a);
            edited |= optional_f64(ui, (device_id, "ieee-inst-s"), "Inst time", inst_s);
        }
        CurveSpec::Iec { kind, pickup_a, tms, inst_a, inst_s } => {
            ui.label(RichText::new(format!("IEC {}", iec_name(*kind))).strong());
            edited |= drag_value(ui, "Pickup", pickup_a, 10.0, 0.0..=1.0e9, " A");
            edited |= drag_value(ui, "TMS", tms, 0.01, 0.0..=1.0e4, "");
            ui.label(RichText::new("Empty optional fields are cleared.").color(color("#94A3B8")));
            edited |= optional_f64(ui, (device_id, "iec-inst-a"), "Inst", inst_a);
            edited |= optional_f64(ui, (device_id, "iec-inst-s"), "Inst time", inst_s);
        }
        CurveSpec::ThermalMagnetic { lt_pickup_a, lt_delay_s, st_pickup_a, st_delay_s, inst_a, inst_s } => {
            ui.label(RichText::new("THERMAL MAGNETIC").strong());
            edited |= drag_value(ui, "LT pickup", lt_pickup_a, 10.0, 0.0..=1.0e9, " A");
            edited |= drag_value(ui, "LT delay", lt_delay_s, 0.001, 0.0..=1.0e6, " s");
            ui.label(RichText::new("Empty optional fields are cleared.").color(color("#94A3B8")));
            edited |= optional_f64(ui, (device_id, "tm-st-a"), "ST pickup", st_pickup_a);
            edited |= optional_f64(ui, (device_id, "tm-st-s"), "ST delay", st_delay_s);
            edited |= optional_f64(ui, (device_id, "tm-inst-a"), "Inst", inst_a);
            edited |= optional_f64(ui, (device_id, "tm-inst-s"), "Inst time", inst_s);
        }
        CurveSpec::Points { points, max_points, beyond_last, band_interpretation } => {
            ui.label(RichText::new("POINTS").strong());
            ui.label(format!("{} points", points.len()));
            ui.label(if max_points.is_some() { "max_points present" } else { "max_points absent" });
            ui.label(format!("beyond_last {}", tail_name(*beyond_last)));
            ui.label(format!("band_interpretation {}", band_name(*band_interpretation)));
        }
        CurveSpec::SettingsNotCollected { note } => {
            ui.label(RichText::new("SETTINGS NOT COLLECTED").strong());
            if note.is_empty() {
                ui.label(RichText::new("No note.").color(color("#94A3B8")));
            } else {
                ui.label(note.as_str());
            }
        }
    }
    edited
}

fn show_library_device(ui: &mut egui::Ui, device: &flashmob::library::LibraryDevice) {
    ui.separator();
    ui.label(RichText::new(format!("{}  {}  {}", device.id, device.manufacturer, device.style)).strong().color(color("#E2E8F0")));
    ui.label(RichText::new(&device.category).color(color("#94A3B8")));
    ui.label(RichText::new("Trip units").strong());
    if device.trip_units.is_empty() {
        ui.label(RichText::new("none").color(color("#94A3B8")));
    }
    for unit in &device.trip_units {
        ui.label(format!("Rating {} A", format_num(unit.rating_a)));
        for function in &unit.functions {
            let name = if function.name.is_empty() { "(unnamed)" } else { function.name.as_str() };
            ui.label(RichText::new(format!("  {name}  {}", function.kind)).color(color("#94A3B8")));
        }
    }
    ui.label(RichText::new("Frames").strong());
    if device.frames.is_empty() {
        ui.label(RichText::new("none").color(color("#94A3B8")));
    }
    for frame in &device.frames {
        let sensors = if frame.sensors_a.is_empty() {
            "none".to_string()
        } else {
            frame.sensors_a.iter().map(|amps| format_num(*amps)).collect::<Vec<_>>().join(", ")
        };
        ui.label(format!("Frame {} A  sensors {sensors}", format_num(frame.rating_a)));
    }
}

fn duty_grid(ui: &mut egui::Ui, rows: &[InterruptingDuty]) {
    egui::Grid::new("interrupting-duty").striped(true).show(ui, |ui| {
        for heading in ["Id", "Name", "Bus", "Sym kA", "Rating kA", "Status"] {
            ui.label(RichText::new(heading).strong());
        }
        ui.end_row();
        for row in rows {
            let ink = duty_color(&row.status);
            let rating = match row.adjusted_symmetrical_ka {
                Some(ka) => format!("{ka:.3} adjusted"),
                None => format!("{:.3} rated", row.rated_symmetrical_ka),
            };
            ui.label(RichText::new(&row.id).color(ink)).on_hover_text(&row.note);
            ui.label(RichText::new(&row.name).color(ink)).on_hover_text(&row.note);
            ui.label(RichText::new(&row.bus_id).color(ink)).on_hover_text(&row.note);
            ui.label(RichText::new(format!("{:.3}", row.symmetrical_ka)).color(ink)).on_hover_text(&row.note);
            ui.label(RichText::new(rating).color(ink)).on_hover_text(&row.note);
            ui.label(RichText::new(&row.status).color(ink)).on_hover_text(&row.note);
            ui.end_row();
        }
    });
}

fn duty_label(ui: &mut egui::Ui, row: &InterruptingDuty) {
    let rating = match row.adjusted_symmetrical_ka {
        Some(ka) => format!("{ka:.3} kA adjusted"),
        None => format!("{:.3} kA rated", row.rated_symmetrical_ka),
    };
    let text = format!("{}  {:.3} kA symmetrical  {rating}  {}", row.id, row.symmetrical_ka, row.status);
    ui.label(RichText::new(text).color(duty_color(&row.status))).on_hover_text(&row.note);
}

fn duty_color(status: &str) -> Color32 {
    match status {
        "exceeds_rating" | "not_adjusted" => color("#FFB300"),
        _ => color("#E2E8F0"),
    }
}

fn ieee_name(kind: IeeeKind) -> &'static str {
    match kind {
        IeeeKind::ModeratelyInverse => "moderately inverse",
        IeeeKind::VeryInverse => "very inverse",
        IeeeKind::ExtremelyInverse => "extremely inverse",
    }
}

fn iec_name(kind: IecKind) -> &'static str {
    match kind {
        IecKind::Si => "SI",
        IecKind::Vi => "VI",
        IecKind::Ei => "EI",
        IecKind::Lti => "LTI",
    }
}

fn tail_name(tail: PointTail) -> &'static str {
    match tail {
        PointTail::HoldLastTime => "hold_last_time",
        PointTail::Unavailable => "unavailable",
    }
}

fn band_name(band: BandInterpretation) -> &'static str {
    match band {
        BandInterpretation::Entered => "entered",
        BandInterpretation::Assumed => "assumed",
    }
}

fn format_num(value: f64) -> String {
    if !value.is_finite() {
        return value.to_string();
    }
    let text = format!("{value:.6}");
    if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        text
    }
}

fn format_num_opt(value: Option<f64>) -> String {
    value.map(format_num).unwrap_or_default()
}

fn parse_f64(text: &str) -> Option<f64> {
    let value: f64 = text.parse().ok()?;
    value.is_finite().then_some(value)
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
    if bus.source && bus.source_out_of_service {
        let grey = Stroke::new(stroke.width, color("#64748B"));
        painter.circle_stroke(map(bus.x - 112.0, bus.y), 16.0 * z, grey);
        if diagram_detail(z) != DiagramDetail::Overview {
            let label = Rect::from_min_size(map(bus.x - 150.0, bus.y + 22.0), Vec2::new(150.0, 14.0) * z);
            paint_diagram_line(painter, label, "SOURCE OUT OF SERVICE", 10.0 * z, color("#64748B"));
        }
    } else if bus.source {
        painter.circle_stroke(map(bus.x - 112.0, bus.y), 16.0 * z, stroke);
    }
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
    for worst in &bus.worst {
        let hot = !worst.complete || worst.cal_cm2.is_some_and(|c| c >= 8.0);
        line(worst.label(), size, if hot { color("#FFB300") } else { data });
    }
    if !bus.arc_failures.is_empty() { line("ARC FLASH FAILED".into(), size, color("#FFB300")); }
}

/// Disconnect switch: two contacts and a blade, in line when closed, angled and amber when open.
fn draw_switch(painter: &egui::Painter, map: impl Fn(f64, f64) -> Pos2, x: f64, y: f64, closed: bool, z: f32) {
    let ink = if closed { color("#E2E8F0") } else { color("#FFB300") };
    painter.rect_filled(Rect::from_center_size(map(x, y), Vec2::new(14.0, 18.0) * z), 0.0, color("#0A0C10"));
    painter.circle_filled(map(x, y - 7.0), (2.0 * z).max(1.0), color("#E2E8F0"));
    painter.circle_filled(map(x, y + 7.0), (2.0 * z).max(1.0), color("#E2E8F0"));
    let tip = if closed { map(x, y - 7.0) } else { map(x + 9.0, y - 5.0) };
    painter.line_segment([map(x, y + 7.0), tip], Stroke::new((1.6 * z).clamp(0.8, 2.0), ink));
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

/// Dark-theme TCC palette (the SVG export uses `tcc::SVG_COLORS` on a light background).
const GUI_CURVE_COLORS: [&str; 6] = ["#E2E8F0", "#00FF66", "#FFB300", "#38BDF8", "#F472B6", "#A3E635"];
const GUI_FAULT_MARKER: &str = "#94A3B8";
const GUI_ARC_MARKER: &str = "#FB923C";

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
        // The SVG marker colors are for a light background; use the dark-theme set here.
        let (ink, dash) = match marker.kind {
            tcc::MarkerKind::Fault => (color(GUI_FAULT_MARKER), 4.0),
            tcc::MarkerKind::Arc => (color(GUI_ARC_MARKER), 4.0),
            tcc::MarkerKind::ArcAssumed => (color(GUI_ARC_MARKER), 1.5),
        };
        painter.extend(egui::Shape::dashed_line(
            &[Pos2::new(x, plot_rect.top()), Pos2::new(x, plot_rect.bottom())],
            Stroke::new(1.2, ink),
            dash,
            3.0,
        ));
    }
    for (n, curve) in plot.curves.iter().enumerate() {
        let ink = color(GUI_CURVE_COLORS[n % GUI_CURVE_COLORS.len()]);
        let mut points = Vec::new();
        for (amp, time) in &curve.points {
            if *amp < plot.i_min || *amp > plot.i_max || *time < plot.t_min || *time > plot.t_max {
                if points.len() > 1 {
                    painter.add(egui::Shape::line(points.clone(), Stroke::new(2.2, ink)));
                }
                points.clear();
                continue;
            }
            points.push(Pos2::new(x_of(*amp), y_of(*time)));
        }
        if points.len() > 1 {
            painter.add(egui::Shape::line(points, Stroke::new(2.2, ink)));
        }
    }
    let mut y = plot_rect.top();
    for (n, curve) in plot.curves.iter().enumerate() {
        let ink = color(GUI_CURVE_COLORS[n % GUI_CURVE_COLORS.len()]);
        painter.line_segment([Pos2::new(plot_rect.right() + 16.0, y + 8.0), Pos2::new(plot_rect.right() + 36.0, y + 8.0)], Stroke::new(2.4, ink));
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

fn write_status(path: &std::path::Path, text: &str) -> Result<String, String> {
    match std::fs::write(path, text) {
        Ok(()) => Ok(format!("Wrote {}", path.display())),
        Err(err) => Err(format!("could not write {}: {err}", path.display())),
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
