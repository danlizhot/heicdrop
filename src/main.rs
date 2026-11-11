// Pas de console noire derrière la fenêtre en mode release sous Windows.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod converter;

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Arc, mpsc};
use std::time::Duration;

use converter::{ConvResult, INPUT_EXTENSIONS, OutputFormat, Progress, collect_images, convert_batch};
use eframe::egui::{self, Color32, CornerRadius, RichText, Stroke};

const APP_NAME: &str = "HeicDrop";
const ACCENT: Color32 = Color32::from_rgb(37, 99, 235);

struct Job {
    total: usize,
    out_dir: PathBuf,
    progress: Arc<Progress>,
    rx: mpsc::Receiver<Vec<ConvResult>>,
}

struct Summary {
    total: usize,
    ok: usize,
    errors: Vec<(String, String)>,
    cancelled: bool,
    out_dir: PathBuf,
}

struct App {
    files: Vec<PathBuf>,
    format: OutputFormat,
    merge_pdf: bool,
    last_dir: Option<PathBuf>,
    job: Option<Job>,
    summary: Option<Summary>,
    status: String,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>, initial: Vec<PathBuf>) -> Self {
        let mut style = (*cc.egui_ctx.global_style()).clone();
        style.spacing.item_spacing = egui::vec2(8.0, 8.0);
        style.spacing.button_padding = egui::vec2(10.0, 5.0);
        for (_, font) in style.text_styles.iter_mut() {
            font.size *= 1.1;
        }
        cc.egui_ctx.set_global_style(style);
        cc.egui_ctx.set_visuals(egui::Visuals::light());

        let storage = cc.storage;
        let format = storage
            .and_then(|s| s.get_string("format"))
            .and_then(|f| OutputFormat::from_label(&f))
            .unwrap_or(OutputFormat::Jpeg);
        let merge_pdf = storage.and_then(|s| s.get_string("merge_pdf")).as_deref() == Some("1");
        let last_dir = storage.and_then(|s| s.get_string("last_dir")).map(PathBuf::from);

        let mut app = Self {
            files: Vec::new(),
            format,
            merge_pdf,
            last_dir,
            job: None,
            summary: None,
            status: "Prêt.".into(),
        };
        app.add_paths(initial);
        app
    }

    fn busy(&self) -> bool {
        self.job.is_some()
    }

    fn add_paths<P: AsRef<Path>>(&mut self, paths: impl IntoIterator<Item = P>) {
        if self.busy() {
            return;
        }
        let paths: Vec<PathBuf> = paths.into_iter().map(|p| p.as_ref().to_path_buf()).collect();
        if paths.is_empty() {
            return;
        }
        let known: std::collections::HashSet<PathBuf> =
            self.files.iter().map(|p| std::fs::canonicalize(p).unwrap_or_else(|_| p.clone())).collect();
        let new: Vec<PathBuf> = collect_images(&paths)
            .into_iter()
            .filter(|p| !known.contains(&std::fs::canonicalize(p).unwrap_or_else(|_| p.clone())))
            .collect();
        self.status = if new.is_empty() {
            "Aucune nouvelle image prise en charge.".into()
        } else {
            format!("{} image(s) ajoutée(s).", new.len())
        };
        self.files.extend(new);
    }

    fn browse_files(&mut self) {
        let exts: Vec<&str> = INPUT_EXTENSIONS.to_vec();
        if let Some(files) = rfd::FileDialog::new()
            .set_title("Choisir des images")
            .add_filter("Images", &exts)
            .add_filter("Photos HEIC", &["heic", "heif"])
            .add_filter("Tous les fichiers", &["*"])
            .pick_files()
        {
            self.add_paths(files);
        }
    }

    fn browse_folder(&mut self) {
        if let Some(dir) = rfd::FileDialog::new().set_title("Choisir un dossier d'images").pick_folder() {
            self.add_paths([dir]);
        }
    }

    fn start(&mut self, ctx: &egui::Context) {
        if self.files.is_empty() {
            self.status = "Ajoutez d'abord des images à convertir.".into();
            return;
        }
        let start_dir = self
            .last_dir
            .clone()
            .filter(|d| d.is_dir())
            .or_else(|| self.files[0].parent().map(Path::to_path_buf));
        let mut dialog = rfd::FileDialog::new().set_title("Où enregistrer les images converties ?");
        if let Some(d) = start_dir {
            dialog = dialog.set_directory(d);
        }
        let Some(out_dir) = dialog.pick_folder() else { return };
        self.last_dir = Some(out_dir.clone());

        let files = self.files.clone();
        let fmt = self.format;
        let merge = self.merge_pdf && fmt == OutputFormat::Pdf;
        let progress = Arc::new(Progress::default());
        let (tx, rx) = mpsc::channel();
        {
            let progress = progress.clone();
            let out_dir = out_dir.clone();
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                let res = convert_batch(&files, &out_dir, fmt, merge, &progress);
                let _ = tx.send(res);
                ctx.request_repaint();
            });
        }
        self.job = Some(Job { total: self.files.len(), out_dir, progress, rx });
    }

    fn poll_job(&mut self) {
        let Some(job) = &self.job else { return };
        let Ok(results) = job.rx.try_recv() else { return };
        let ok = results.iter().filter(|r| r.ok()).count();
        let errors: Vec<(String, String)> = results
            .iter()
            .filter_map(|r| {
                r.error.as_ref().map(|e| {
                    (r.source.file_name().unwrap_or_default().to_string_lossy().into_owned(), e.clone())
                })
            })
            .collect();
        let cancelled = results.len() < job.total;
        let summary = Summary { total: job.total, ok, errors, cancelled, out_dir: job.out_dir.clone() };
        self.status = format!("{} image(s) convertie(s) sur {}.", ok, job.total);

        // Retire de la liste les images converties avec succès.
        let done: std::collections::HashSet<&PathBuf> =
            results.iter().filter(|r| r.ok()).map(|r| &r.source).collect();
        self.files.retain(|f| !done.contains(f));

        self.summary = Some(summary);
        self.job = None;
    }

    // ------------------------------------------------------------------ UI

    fn drop_zone(&mut self, ui: &mut egui::Ui, height: f32) {
        let hovering = ui.ctx().input(|i| !i.raw.hovered_files.is_empty());
        let (fill, stroke) = if hovering {
            (Color32::from_rgb(219, 234, 254), Stroke::new(2.0, ACCENT))
        } else {
            (Color32::from_rgb(241, 245, 249), Stroke::new(1.5, Color32::from_rgb(148, 163, 184)))
        };

        let frame = egui::Frame::new()
            .fill(fill)
            .stroke(stroke)
            .corner_radius(CornerRadius::same(10))
            .inner_margin(egui::Margin::same(10));

        frame.show(ui, |ui| {
            ui.set_min_height(height);
            ui.set_width(ui.available_width());
            if self.files.is_empty() {
                let rect = ui.max_rect();
                let resp = ui.interact(rect, egui::Id::unique("drop_hint"), egui::Sense::click());
                let painter = ui.painter();
                let c = rect.center();
                // Petite illustration : une photo stylisée
                let pic = egui::Rect::from_center_size(c - egui::vec2(0.0, 70.0), egui::vec2(96.0, 72.0));
                let col = if hovering { ACCENT } else { Color32::from_rgb(100, 116, 139) };
                painter.rect_stroke(pic, CornerRadius::same(10), Stroke::new(3.0, col), egui::StrokeKind::Inside);
                painter.circle_filled(pic.right_top() + egui::vec2(-24.0, 22.0), 8.0, col);
                let b = pic.bottom() - 12.0;
                painter.add(egui::Shape::convex_polygon(
                    vec![
                        egui::pos2(pic.left() + 12.0, b),
                        egui::pos2(pic.left() + 38.0, pic.top() + 28.0),
                        egui::pos2(pic.left() + 60.0, b),
                    ],
                    col,
                    Stroke::NONE,
                ));
                painter.add(egui::Shape::convex_polygon(
                    vec![
                        egui::pos2(pic.left() + 48.0, b),
                        egui::pos2(pic.left() + 66.0, pic.top() + 40.0),
                        egui::pos2(pic.right() - 12.0, b),
                    ],
                    col,
                    Stroke::NONE,
                ));
                let text_col = Color32::from_rgb(30, 41, 59);
                painter.text(c + egui::vec2(0.0, 10.0), egui::Align2::CENTER_CENTER,
                    "Glissez-déposez vos photos ici", egui::FontId::proportional(22.0), text_col);
                painter.text(c + egui::vec2(0.0, 40.0), egui::Align2::CENTER_CENTER,
                    "fichiers ou dossiers entiers", egui::FontId::proportional(15.0), Color32::from_rgb(100, 116, 139));
                painter.text(c + egui::vec2(0.0, 76.0), egui::Align2::CENTER_CENTER,
                    "ou cliquez ici pour parcourir", egui::FontId::proportional(15.0), ACCENT);
                if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() && !self.busy() {
                    self.browse_files();
                }
            } else {
                let mut remove = None;
                egui::ScrollArea::vertical().auto_shrink([false, false]).max_height(height).show(ui, |ui| {
                    for (i, f) in self.files.iter().enumerate() {
                        ui.horizontal(|ui| {
                            if ui
                                .add_enabled(!self.busy(), egui::Button::new(RichText::new("×").size(18.0).color(Color32::from_rgb(148, 163, 184))).frame(false))
                                .on_hover_text("Retirer")
                                .clicked()
                            {
                                remove = Some(i);
                            }
                            ui.label(RichText::new(f.file_name().unwrap_or_default().to_string_lossy()).strong());
                            ui.label(
                                RichText::new(f.parent().map(|p| p.display().to_string()).unwrap_or_default())
                                    .weak()
                                    .small(),
                            );
                        });
                    }
                });
                if let Some(i) = remove {
                    self.files.remove(i);
                }
            }
        });
    }

    fn summary_window(&mut self, ctx: &egui::Context) {
        let Some(s) = &self.summary else { return };
        let mut close = false;
        let mut open_dir = false;
        egui::Modal::new(egui::Id::unique("summary")).show(ctx, |ui| {
            ui.set_width(420.0);
            let title = if s.errors.is_empty() && !s.cancelled { "Conversion terminée" } else { "Conversion terminée" };
            ui.heading(title);
            ui.label(format!("{} image(s) convertie(s) sur {}.", s.ok, s.total));
            if s.cancelled {
                ui.colored_label(Color32::from_rgb(180, 83, 9), "Conversion annulée avant la fin.");
            }
            if !s.errors.is_empty() {
                ui.colored_label(Color32::from_rgb(185, 28, 28), format!("{} erreur(s) :", s.errors.len()));
                egui::ScrollArea::vertical().max_height(160.0).show(ui, |ui| {
                    for (name, err) in &s.errors {
                        ui.label(RichText::new(format!("• {name} : {err}")).small());
                    }
                });
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if s.ok > 0 && ui.button("Ouvrir le dossier").clicked() {
                    open_dir = true;
                    close = true;
                }
                if ui.button("Fermer").clicked() {
                    close = true;
                }
            });
        });
        if open_dir {
            open_folder(&s.out_dir);
        }
        if close {
            self.summary = None;
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.poll_job();

        // Fichiers déposés sur la fenêtre
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).collect());
        if !dropped.is_empty() {
            self.add_paths(dropped);
        }

        egui::Panel::bottom("controls")
            .frame(egui::Frame::new().fill(ctx.global_style().visuals.panel_fill).inner_margin(egui::Margin::symmetric(12, 10)))
            .show(ui, |ui| {
            let busy = self.busy();
            ui.horizontal(|ui| {
                let n = self.files.len();
                ui.label(format!("{n} image{}", if n > 1 { "s" } else { "" }));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add_enabled(!busy && n > 0, egui::Button::new("Vider")).clicked() {
                        self.files.clear();
                        self.status = "Prêt.".into();
                    }
                    if ui.add_enabled(!busy, egui::Button::new("Ajouter un dossier…")).clicked() {
                        self.browse_folder();
                    }
                    if ui.add_enabled(!busy, egui::Button::new("Ajouter des fichiers…")).clicked() {
                        self.browse_files();
                    }
                });
            });

            ui.separator();
            ui.horizontal(|ui| {
                ui.label(RichText::new("Format de sortie :").strong());
                ui.add_enabled_ui(!busy, |ui| {
                    egui::ComboBox::from_id_salt("format")
                        .selected_text(self.format.label())
                        .width(90.0).height(400.0)
                        .show_ui(ui, |ui| {
                            for f in OutputFormat::ALL {
                                ui.selectable_value(&mut self.format, f, f.label());
                            }
                        });
                    if self.format == OutputFormat::Pdf {
                        ui.checkbox(&mut self.merge_pdf, "Regrouper dans un seul PDF");
                    }
                });
            });

            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let btn_w = 140.0;
                let bar_w = (ui.available_width() - btn_w - 12.0).max(50.0);
                let (fraction, text) = match &self.job {
                    Some(j) => {
                        let done = j.progress.done.load(Ordering::Relaxed);
                        (done as f32 / j.total.max(1) as f32, format!("{done} / {}", j.total))
                    }
                    None => (if self.summary.is_some() { 1.0 } else { 0.0 }, String::new()),
                };
                if self.job.is_some() || self.summary.is_some() {
                    ui.add_sized(
                        [bar_w, 28.0],
                        egui::ProgressBar::new(fraction).text(text).fill(ACCENT).corner_radius(4.0),
                    );
                } else {
                    ui.add_space(bar_w + ui.spacing().item_spacing.x);
                }
                if let Some(j) = &self.job {
                    let cancelling = j.progress.cancel.load(Ordering::Relaxed);
                    let b = egui::Button::new(RichText::new(if cancelling { "Annulation…" } else { "Annuler" }).size(16.0));
                    if ui.add_enabled(!cancelling, b).clicked() {
                        j.progress.cancel.store(true, Ordering::Relaxed);
                    }
                } else {
                    let b = egui::Button::new(RichText::new("Convertir").size(16.0).strong().color(Color32::WHITE))
                        .fill(ACCENT)
                        .min_size(egui::vec2(btn_w, 28.0));
                    if ui.add_enabled(!self.files.is_empty(), b).clicked() {
                        self.start(&ctx);
                    }
                }
            });

            let status = match &self.job {
                Some(j) => format!("Conversion : {}", j.progress.current.lock().unwrap()),
                None => self.status.clone(),
            };
            ui.label(RichText::new(status).weak());
            });

        egui::CentralPanel::default().show(ui, |ui| {
            ui.add_space(4.0);
            ui.heading(RichText::new("Convertisseur d'images").strong());
            ui.label(RichText::new("HEIC, HEIF, PNG, JPEG, WEBP, TIFF… vers JPEG, PNG, PDF et plus.").weak());
            ui.add_space(4.0);
            let h = ui.available_height() - 22.0;
            self.drop_zone(ui, h);
        });

        self.summary_window(&ctx);

        if self.busy() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        storage.set_string("format", self.format.label().to_string());
        storage.set_string("merge_pdf", if self.merge_pdf { "1" } else { "0" }.to_string());
        if let Some(d) = &self.last_dir {
            storage.set_string("last_dir", d.display().to_string());
        }
    }
}

fn open_folder(path: &Path) {
    #[cfg(windows)]
    let cmd = "explorer";
    #[cfg(target_os = "macos")]
    let cmd = "open";
    #[cfg(all(unix, not(target_os = "macos")))]
    let cmd = "xdg-open";
    let _ = std::process::Command::new(cmd).arg(path).spawn();
}

fn load_icon() -> Option<egui::IconData> {
    let bytes = include_bytes!("../assets/icon.png");
    let img = image::load_from_memory(bytes).ok()?.to_rgba8();
    Some(egui::IconData { width: img.width(), height: img.height(), rgba: img.into_raw() })
}

fn main() -> eframe::Result {
    // Fichiers passés en argument (glissés sur l'icône de l'exe, « Ouvrir avec », etc.)
    let initial: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();

    let mut viewport = egui::ViewportBuilder::default()
        .with_title(APP_NAME)
        .with_inner_size([680.0, 560.0])
        .with_min_inner_size([520.0, 440.0])
        .with_drag_and_drop(true);
    if let Some(icon) = load_icon() {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    eframe::run_native(APP_NAME, options, Box::new(|cc| Ok(Box::new(App::new(cc, initial)))))
}
