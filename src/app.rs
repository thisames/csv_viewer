//! Aplicativo: abas, barras, diálogos, atalhos e notificações.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use egui::{
    Align, Align2, Color32, CornerRadius, Frame, Id, Key, KeyboardShortcut, LayerId, Layout,
    Margin, Modifiers, Order, Rect, RichText, Sense, Stroke, StrokeKind, TextEdit, Ui, pos2, vec2,
};
use egui_phosphor::regular as ic;
use encoding_rs::Encoding;
use serde::{Deserialize, Serialize};

use crate::data::export::{self, ExportStyle};
use crate::data::loader::{self, LoadOptions};
use crate::data::table::{FileFormat, delimiter_label, delimiter_symbol};
use crate::document::{DocState, Document};
use crate::format;
use crate::icon;
use crate::theme::{self, Palette};
use crate::ui::grid::{self, CellPos, GridAction, GridCtx};
use crate::ui::{sidebar, welcome, widgets};

const APP_TITLE: &str = "Visualizador de Planilhas";
const MAX_RECENT: usize = 12;

#[derive(Serialize, Deserialize)]
#[serde(default)]
struct Settings {
    dark: bool,
    compact: bool,
    zebra: bool,
    sidebar: bool,
    recent: Vec<PathBuf>,
    last_dir: Option<PathBuf>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            dark: true,
            compact: false,
            zebra: false,
            sidebar: true,
            recent: Vec::new(),
            last_dir: None,
        }
    }
}

enum DialogResult {
    Open(Vec<PathBuf>),
    Export {
        doc_id: u64,
        path: PathBuf,
        style: ExportStyle,
    },
    Cancelled,
}

struct Toast {
    text: String,
    icon: &'static str,
    error: bool,
    created: Instant,
}

struct CellModal {
    title: String,
    subtitle: String,
    value: String,
    pretty: Option<String>,
    show_pretty: bool,
}

struct ImportModal {
    doc_id: u64,
    has_header: bool,
    delimiter: Option<u8>,
    encoding: Option<&'static Encoding>,
}

enum Modal {
    Cell(CellModal),
    Import(ImportModal),
    Shortcuts,
}

pub struct App {
    docs: Vec<Document>,
    active: usize,
    next_id: u64,
    settings: Settings,
    dialog_rx: Option<mpsc::Receiver<DialogResult>>,
    export_rx: Vec<mpsc::Receiver<Result<String, String>>>,
    toasts: Vec<Toast>,
    modal: Option<Modal>,
    focus_search: bool,
    logo: egui::TextureHandle,
    last_title: String,
    #[cfg(feature = "devshot")]
    devshot: Option<crate::devshot::DevShot>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, files: Vec<PathBuf>) -> Self {
        let settings: Settings = cc
            .storage
            .and_then(|s| eframe::get_value(s, eframe::APP_KEY))
            .unwrap_or_default();
        let ctx = &cc.egui_ctx;
        theme::install_fonts(ctx);
        theme::apply(ctx, settings.dark);
        let mut app = Self {
            docs: Vec::new(),
            active: 0,
            next_id: 1,
            settings,
            dialog_rx: None,
            export_rx: Vec::new(),
            toasts: Vec::new(),
            modal: None,
            focus_search: false,
            logo: icon::texture(ctx),
            last_title: String::new(),
            #[cfg(feature = "devshot")]
            devshot: crate::devshot::DevShot::from_env(),
        };
        for f in files {
            app.open_path(f, ctx);
        }
        app
    }

    fn active_doc(&self) -> Option<&Document> {
        self.docs.get(self.active)
    }

    fn active_doc_mut(&mut self) -> Option<&mut Document> {
        self.docs.get_mut(self.active)
    }

    fn ready_doc(&self) -> Option<&Document> {
        self.active_doc().filter(|d| d.is_ready())
    }

    fn toast(&mut self, icon: &'static str, text: impl Into<String>) {
        self.toasts.push(Toast {
            text: text.into(),
            icon,
            error: false,
            created: Instant::now(),
        });
        if self.toasts.len() > 3 {
            self.toasts.remove(0);
        }
    }

    fn toast_error(&mut self, text: impl Into<String>) {
        self.toasts.push(Toast {
            text: text.into(),
            icon: ic::WARNING,
            error: true,
            created: Instant::now(),
        });
    }

    fn remember_recent(&mut self, path: &Path) {
        let abs = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        self.settings.recent.retain(|p| p != &abs);
        self.settings.recent.insert(0, abs.clone());
        self.settings.recent.truncate(MAX_RECENT);
        self.settings.last_dir = abs.parent().map(Path::to_path_buf);
    }

    pub fn open_path(&mut self, path: PathBuf, ctx: &egui::Context) {
        if let Some(i) = self.docs.iter().position(|d| d.same_path(&path)) {
            self.active = i;
            return;
        }
        if let Some(dir) = path.parent() {
            self.settings.last_dir = Some(dir.to_path_buf());
        }
        let doc = Document::open(self.next_id, path, LoadOptions::default(), ctx);
        self.next_id += 1;
        self.docs.push(doc);
        self.active = self.docs.len() - 1;
    }

    fn close_doc(&mut self, i: usize) {
        if i >= self.docs.len() {
            return;
        }
        self.docs[i].cancel_loading();
        self.docs.remove(i);
        if self.active >= self.docs.len() {
            self.active = self.docs.len().saturating_sub(1);
        } else if i < self.active {
            self.active -= 1;
        }
    }

    fn open_dialog(&mut self, ctx: &egui::Context) {
        if self.dialog_rx.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        let dir = self.settings.last_dir.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let mut dialog = rfd::FileDialog::new()
                .set_title("Abrir planilha")
                .add_filter(
                    "Planilhas e CSV",
                    &["csv", "tsv", "txt", "tab", "psv", "dat", "xlsx", "xlsm", "xlsb", "xls", "ods"],
                )
                .add_filter("Todos os arquivos", &["*"]);
            if let Some(d) = dir.filter(|d| d.is_dir()) {
                dialog = dialog.set_directory(d);
            }
            let result = match dialog.pick_files() {
                Some(files) if !files.is_empty() => DialogResult::Open(files),
                _ => DialogResult::Cancelled,
            };
            let _ = tx.send(result);
            ctx.request_repaint();
        });
        self.dialog_rx = Some(rx);
    }

    fn export_dialog(&mut self, ctx: &egui::Context, style: ExportStyle) {
        if self.dialog_rx.is_some() {
            return;
        }
        let Some(doc) = self.ready_doc() else { return };
        let stem = doc
            .path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "dados".to_owned());
        let filtered = !doc.view_spec.is_identity();
        let suffix = match (filtered, style) {
            (true, ExportStyle::ExcelBr) => "_filtrado_excel",
            (true, ExportStyle::Standard) => "_filtrado",
            (false, ExportStyle::ExcelBr) => "_excel",
            (false, ExportStyle::Standard) => "_exportado",
        };
        let name = format!("{stem}{suffix}.csv");
        let dir = doc.path.parent().map(Path::to_path_buf);
        let doc_id = doc.id;
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let mut dialog = rfd::FileDialog::new()
                .set_title("Exportar CSV")
                .set_file_name(&name)
                .add_filter("CSV", &["csv"]);
            if let Some(d) = dir.filter(|d| d.is_dir()) {
                dialog = dialog.set_directory(d);
            }
            let result = match dialog.save_file() {
                Some(path) => DialogResult::Export { doc_id, path, style },
                None => DialogResult::Cancelled,
            };
            let _ = tx.send(result);
            ctx.request_repaint();
        });
        self.dialog_rx = Some(rx);
    }

    fn poll_dialogs(&mut self, ctx: &egui::Context) {
        if let Some(rx) = &self.dialog_rx {
            match rx.try_recv() {
                Ok(result) => {
                    self.dialog_rx = None;
                    match result {
                        DialogResult::Open(files) => {
                            for f in files {
                                self.open_path(f, ctx);
                            }
                        }
                        DialogResult::Export { doc_id, path, style } => self.start_export(ctx, doc_id, path, style),
                        DialogResult::Cancelled => {}
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => self.dialog_rx = None,
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        let mut done = Vec::new();
        self.export_rx.retain(|rx| match rx.try_recv() {
            Ok(r) => {
                done.push(r);
                false
            }
            Err(mpsc::TryRecvError::Empty) => true,
            Err(mpsc::TryRecvError::Disconnected) => false,
        });
        for r in done {
            match r {
                Ok(msg) => self.toast(ic::CHECK_CIRCLE, msg),
                Err(e) => self.toast_error(e),
            }
        }
    }

    fn start_export(&mut self, ctx: &egui::Context, doc_id: u64, path: PathBuf, style: ExportStyle) {
        let Some(doc) = self.docs.iter().find(|d| d.id == doc_id) else { return };
        let Some(table) = doc.table.clone() else { return };
        let view = doc.view.clone();
        let cols = doc.visible_cols();
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let r = export::export_csv(&table, &view, &cols, &path, style)
                .map(|n| format!("Exportado {} ({})", name, format::plural(n, "linha", "linhas")));
            let _ = tx.send(r);
            ctx.request_repaint();
        });
        self.export_rx.push(rx);
        self.toast(ic::EXPORT, "Exportando…");
    }

    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .filter(|p| !p.as_os_str().is_empty())
                .collect()
        });
        for p in dropped {
            self.open_path(p, ctx);
        }
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        let cmd = Modifiers::COMMAND;
        let sc = |m: Modifiers, k: Key| KeyboardShortcut::new(m, k);
        let (open, close, find, prev, next, side, reload, export, help) = ctx.input_mut(|i| {
            let prev = i.consume_shortcut(&sc(cmd | Modifiers::SHIFT, Key::Tab))
                || i.consume_shortcut(&sc(cmd, Key::PageUp));
            let next = i.consume_shortcut(&sc(cmd, Key::Tab)) || i.consume_shortcut(&sc(cmd, Key::PageDown));
            (
                i.consume_shortcut(&sc(cmd, Key::O)),
                i.consume_shortcut(&sc(cmd, Key::W)),
                i.consume_shortcut(&sc(cmd, Key::F)),
                prev,
                next,
                i.consume_shortcut(&sc(cmd, Key::B)),
                i.consume_shortcut(&sc(cmd, Key::R)) || i.consume_key(Modifiers::NONE, Key::F5),
                i.consume_shortcut(&sc(cmd, Key::E)),
                i.consume_key(Modifiers::NONE, Key::F1),
            )
        });
        if open {
            self.open_dialog(ctx);
        }
        if close && !self.docs.is_empty() {
            self.close_doc(self.active);
        }
        if find && self.ready_doc().is_some() {
            self.focus_search = true;
        }
        if !self.docs.is_empty() {
            let n = self.docs.len();
            if next {
                self.active = (self.active + 1) % n;
            }
            if prev {
                self.active = (self.active + n - 1) % n;
            }
        }
        if side {
            self.settings.sidebar = !self.settings.sidebar;
        }
        if reload && let Some(doc) = self.active_doc_mut() {
            let opts = doc.opts.clone();
            doc.reload(opts, ctx);
        }
        if export && self.ready_doc().is_some() {
            self.export_dialog(ctx, ExportStyle::Standard);
        }
        if help {
            self.modal = Some(Modal::Shortcuts);
        }
    }

    fn update_title(&mut self, ctx: &egui::Context) {
        let title = match self.active_doc() {
            Some(d) => format!("{} — {APP_TITLE}", d.title),
            None => APP_TITLE.to_owned(),
        };
        if title != self.last_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.last_title = title;
        }
    }

    fn set_dark(&mut self, ctx: &egui::Context, dark: bool) {
        self.settings.dark = dark;
        ctx.set_theme(if dark { egui::Theme::Dark } else { egui::Theme::Light });
    }

    // ------------------------------------------------------------------ barras

    fn top_bar(&mut self, ui: &mut Ui) {
        let p = Palette::of(ui);
        let ctx = ui.ctx().clone();
        egui::Panel::top("topbar")
            .exact_size(52.0)
            .show_separator_line(false)
            .frame(Frame::new().fill(p.panel).inner_margin(Margin::symmetric(12, 0)))
            .show(ui, |ui| {
                let r = ui.max_rect();
                ui.painter().hline(
                    r.x_range().expand(12.0),
                    r.max.y - 0.5,
                    Stroke::new(1.0, p.border),
                );
                ui.horizontal_centered(|ui| {
                    widgets::logo(ui, &self.logo, 26.0).on_hover_text(APP_TITLE);
                    ui.add_space(6.0);

                    let ready = self.ready_doc().is_some();
                    let right_w = if ready { 300.0 + 3.0 * 36.0 + 24.0 } else { 2.0 * 36.0 + 16.0 };
                    let tabs_w = (ui.available_width() - right_w).max(100.0);
                    let mut activate = None;
                    let mut close = None;
                    let mut open = false;
                    egui::ScrollArea::horizontal()
                        .id_salt("tabs")
                        .max_width(tabs_w)
                        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                        .show(ui, |ui| {
                            ui.horizontal_centered(|ui| {
                                ui.spacing_mut().item_spacing.x = 4.0;
                                for (i, doc) in self.docs.iter().enumerate() {
                                    let (resp, x) = tab(ui, doc, i == self.active);
                                    if x || resp.middle_clicked() {
                                        close = Some(i);
                                    } else if resp.clicked() {
                                        activate = Some(i);
                                    }
                                }
                                if widgets::icon_button(ui, ic::PLUS, "Abrir arquivo (Ctrl+O)", false).clicked() {
                                    open = true;
                                }
                            });
                        });
                    if let Some(i) = activate {
                        self.active = i;
                    }
                    if let Some(i) = close {
                        self.close_doc(i);
                    }
                    if open {
                        self.open_dialog(&ctx);
                    }

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        let menu_resp = widgets::icon_button(ui, ic::DOTS_THREE_VERTICAL, "Mais opções", false);
                        egui::Popup::menu(&menu_resp)
                            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                            .show(|ui| {
                                ui.set_min_width(250.0);
                                self.main_menu(ui, &ctx);
                            });
                        let dark = self.settings.dark;
                        if widgets::icon_button(
                            ui,
                            if dark { ic::SUN } else { ic::MOON },
                            if dark { "Tema claro" } else { "Tema escuro" },
                            false,
                        )
                        .clicked()
                        {
                            self.set_dark(&ctx, !dark);
                        }
                        if ready {
                            if widgets::icon_button(
                                ui,
                                ic::SIDEBAR_SIMPLE,
                                "Painel de colunas (Ctrl+B)",
                                self.settings.sidebar,
                            )
                            .clicked()
                            {
                                self.settings.sidebar = !self.settings.sidebar;
                            }
                            ui.add_space(6.0);
                            let focus = std::mem::take(&mut self.focus_search);
                            if let Some(doc) = self.active_doc_mut() {
                                search_box(ui, doc, focus);
                            }
                        }
                    });
                });
            });
    }

    fn main_menu(&mut self, ui: &mut Ui, ctx: &egui::Context) {
        let has_doc = self.active_doc().is_some();
        let ready = self.ready_doc().is_some();
        if grid::menu_item(ui, ic::FOLDER_OPEN, "Abrir arquivo…", Some("Ctrl+O")) {
            self.open_dialog(ctx);
            ui.close();
        }
        if has_doc && grid::menu_item(ui, ic::ARROW_COUNTER_CLOCKWISE, "Recarregar arquivo", Some("Ctrl+R")) {
            if let Some(doc) = self.active_doc_mut() {
                let opts = doc.opts.clone();
                doc.reload(opts, ctx);
            }
            ui.close();
        }
        if has_doc && grid::menu_item(ui, ic::SLIDERS_HORIZONTAL, "Opções de importação…", None) {
            self.open_import_modal();
            ui.close();
        }
        if ready {
            ui.separator();
            if grid::menu_item(ui, ic::EXPORT, "Exportar visão atual (CSV)…", Some("Ctrl+E")) {
                self.export_dialog(ctx, ExportStyle::Standard);
                ui.close();
            }
            if grid::menu_item(ui, ic::MICROSOFT_EXCEL_LOGO, "Exportar para Excel (CSV ;)…", None) {
                self.export_dialog(ctx, ExportStyle::ExcelBr);
                ui.close();
            }
        }
        ui.separator();
        if toggle_item(ui, "Linhas compactas", self.settings.compact) {
            self.settings.compact = !self.settings.compact;
        }
        if toggle_item(ui, "Linhas zebradas", self.settings.zebra) {
            self.settings.zebra = !self.settings.zebra;
        }
        ui.separator();
        if grid::menu_item(ui, ic::KEYBOARD, "Atalhos de teclado", Some("F1")) {
            self.modal = Some(Modal::Shortcuts);
            ui.close();
        }
        if has_doc && grid::menu_item(ui, ic::X, "Fechar aba", Some("Ctrl+W")) {
            self.close_doc(self.active);
            ui.close();
        }
    }

    fn open_import_modal(&mut self) {
        if let Some(doc) = self.active_doc() {
            self.modal = Some(Modal::Import(ImportModal {
                doc_id: doc.id,
                has_header: doc.opts.has_header,
                delimiter: doc.opts.delimiter,
                encoding: doc.opts.encoding,
            }));
        }
    }

    fn status_bar(&mut self, ui: &mut Ui) {
        let p = Palette::of(ui);
        let mut open_import = false;
        egui::Panel::bottom("statusbar")
            .exact_size(30.0)
            .show_separator_line(false)
            .frame(Frame::new().fill(p.panel).inner_margin(Margin::symmetric(12, 0)))
            .show(ui, |ui| {
                let r = ui.max_rect();
                ui.painter().hline(r.x_range().expand(12.0), r.min.y + 0.5, Stroke::new(1.0, p.border));
                let Some(doc) = self.ready_doc() else { return };
                let Some(table) = doc.table.as_ref() else { return };
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    let small = |t: String, c: Color32| RichText::new(t).font(theme::regular(12.0)).color(c);
                    if doc.is_computing() {
                        ui.add(egui::Spinner::new().size(12.0).color(p.accent));
                    } else {
                        let (rect, _) = ui.allocate_exact_size(vec2(8.0, 8.0), Sense::hover());
                        ui.painter().circle_filled(rect.center(), 3.5, p.success);
                    }
                    let total = table.rows;
                    let shown = doc.view.len();
                    let rows = if doc.view_spec.is_identity() || shown == total {
                        format::plural(total, "linha", "linhas")
                    } else {
                        format!("{} de {} linhas", format::int(shown), format::int(total))
                    };
                    ui.label(small(rows, p.text));
                    ui.label(small("·".into(), p.text_faint));
                    let vis = doc.visible_cols().len();
                    let cols_txt = if vis < table.columns.len() {
                        format!("{} de {} colunas", vis, table.columns.len())
                    } else {
                        format::plural(table.columns.len(), "coluna", "colunas")
                    };
                    ui.label(small(cols_txt, p.text_muted));

                    // Seleção.
                    if let Some((_, s)) = &doc.selection_summary {
                        ui.label(small("·".into(), p.text_faint));
                        ui.label(small(format::plural(s.cells, "célula", "células"), p.text_muted));
                        if s.numbers > 0 {
                            for (label, v) in [
                                ("Soma", s.sum),
                                ("Média", s.mean().unwrap_or(0.0)),
                                ("Mín", s.min),
                                ("Máx", s.max),
                            ] {
                                ui.label(small("·".into(), p.text_faint));
                                ui.label(small(label.to_owned(), p.text_faint));
                                ui.label(
                                    RichText::new(format::num(v))
                                        .font(theme::medium(12.0))
                                        .color(p.text),
                                );
                            }
                        } else if s.filled > 0 {
                            ui.label(small(format!("Preenchidas: {}", format::int(s.filled)), p.text_faint));
                        }
                    } else if let Some(c) = doc.grid.cursor
                        && c.row < doc.view.len()
                    {
                        ui.label(small("·".into(), p.text_faint));
                        let data_row = doc.view.row(c.row);
                        ui.label(small(
                            format!("{}{}", format::column_letter(c.col), data_row + 1),
                            p.accent,
                        ));
                        ui.label(small(table.columns[c.col].name.clone(), p.text_muted));
                    }

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if let Some(info) = &doc.info {
                            ui.label(small(format!("lido em {}", format::duration(info.load_time)), p.text_faint))
                                .on_hover_text(format!(
                                    "Memória aproximada: {}",
                                    format::bytes(table.memory_bytes() as u64)
                                ));
                            ui.label(small(format::bytes(info.file_size), p.text_faint));
                            let mut parts = vec![info.format_label().to_owned()];
                            if let Some(e) = info.encoding {
                                parts.push(e.split(' ').next().unwrap_or(e).to_owned());
                            }
                            if let Some(d) = info.delimiter {
                                parts.push(format!("separador {}", delimiter_symbol(d)));
                            }
                            if let Some(s) = &info.sheet {
                                parts.push(s.clone());
                            }
                            let resp = ui.add(
                                egui::Button::new((
                                    theme::icon_text(ic::SLIDERS_HORIZONTAL, 13.0).color(p.text_faint),
                                    small(parts.join(" · "), p.text_muted),
                                ))
                                .gap(6.0)
                                .frame_when_inactive(false)
                                .min_size(vec2(0.0, 22.0)),
                            );
                            let mut tip = String::from("Opções de importação");
                            if let Some(d) = info.delimiter {
                                tip.push_str(&format!("\nSeparador: {}", delimiter_label(d)));
                            }
                            if info.ragged_rows > 0 {
                                tip.push_str(&format!(
                                    "\n{} com número de campos diferente do cabeçalho",
                                    format::plural(info.ragged_rows, "linha", "linhas")
                                ));
                            }
                            if resp.on_hover_text(tip).clicked() {
                                open_import = true;
                            }
                            if info.ragged_rows > 0 {
                                ui.label(theme::icon_text(ic::WARNING, 15.0).color(p.warning)).on_hover_text(format!(
                                    "{} com número de campos diferente do cabeçalho.\nTalvez o separador esteja errado — clique nas opções de importação.",
                                    format::plural(info.ragged_rows, "linha", "linhas")
                                ));
                            }
                        }
                    });
                });
            });
        if open_import {
            self.open_import_modal();
        }
    }

    fn side_panel(&mut self, ui: &mut Ui) {
        let p = Palette::of(ui);
        egui::Panel::left("sidebar")
            .default_size(300.0)
            .size_range(230.0..=520.0)
            .resizable(true)
            .show_separator_line(true)
            .frame(Frame::new().fill(p.panel).inner_margin(Margin {
                left: 12,
                right: 12,
                top: 0,
                bottom: 0,
            }))
            .show(ui, |ui| {
                if let Some(doc) = self.active_doc_mut() {
                    sidebar::show(ui, doc);
                }
            });
    }

    fn central(&mut self, ui: &mut Ui) {
        let p = Palette::of(ui);
        let ctx = ui.ctx().clone();
        egui::CentralPanel::default()
            .frame(Frame::new().fill(p.bg))
            .show(ui, |ui| {
                if self.docs.is_empty() {
                    if let Some(a) = welcome::show(ui, &self.settings.recent, &self.logo) {
                        match a {
                            welcome::WelcomeAction::Open => self.open_dialog(&ctx),
                            welcome::WelcomeAction::OpenRecent(p) => {
                                if p.exists() {
                                    self.open_path(p, &ctx);
                                } else {
                                    self.settings.recent.retain(|r| r != &p);
                                    self.toast_error("Esse arquivo não existe mais.");
                                }
                            }
                            welcome::WelcomeAction::ForgetRecent(p) => self.settings.recent.retain(|r| r != &p),
                        }
                    }
                    return;
                }
                let active = self.active;
                let doc = &mut self.docs[active];
                match &doc.state {
                    DocState::Loading { progress, started, .. } => {
                        let (progress, started) = (progress.clone(), *started);
                        if welcome::loading(ui, &doc.title, &doc.path, &progress, started) {
                            self.close_doc(active);
                        }
                    }
                    DocState::Failed(msg) => {
                        let msg = msg.clone();
                        match welcome::error(ui, &doc.title, &msg) {
                            Some(welcome::ErrorAction::Retry) => {
                                let opts = doc.opts.clone();
                                doc.reload(opts, &ctx);
                            }
                            Some(welcome::ErrorAction::Options) => self.open_import_modal(),
                            Some(welcome::ErrorAction::Close) => self.close_doc(active),
                            None => {}
                        }
                    }
                    DocState::Ready => self.document_view(ui, &ctx),
                }
            });
    }

    fn document_view(&mut self, ui: &mut Ui, ctx: &egui::Context) {
        let p = Palette::of(ui);
        let compact = self.settings.compact;
        let zebra = self.settings.zebra;
        let active = self.active;
        let doc = &mut self.docs[active];
        let Some(table) = doc.table.clone() else { return };

        // Barra de filtros ativos.
        let searching = !doc.spec.search.trim().is_empty();
        if searching || !doc.spec.filters.is_empty() {
            egui::Panel::top(Id::new(("filters", doc.id)))
                .show_separator_line(false)
                .frame(Frame::new().fill(p.bg).inner_margin(Margin { left: 12, right: 12, top: 8, bottom: 8 }))
                .show(ui, |ui| {
                    let r = ui.max_rect();
                    ui.painter().hline(r.x_range().expand(12.0), r.max.y + 8.0 - 0.5, Stroke::new(1.0, p.border));
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                        widgets::icon_label(ui, ic::FUNNEL, "Filtros", theme::medium(12.5), p.text_muted);
                        ui.add_space(2.0);
                        if searching {
                            let (_, x) = widgets::filter_chip(
                                ui,
                                ic::MAGNIFYING_GLASS,
                                &format!("“{}” em qualquer coluna", doc.spec.search.trim()),
                            );
                            if x {
                                doc.search_input.clear();
                            }
                        }
                        let mut remove = None;
                        for (i, f) in doc.spec.filters.iter().enumerate() {
                            let (icon, label) = sidebar::filter_label(&table, f);
                            let (resp, x) = widgets::filter_chip(ui, icon, &label);
                            if x {
                                remove = Some(i);
                            }
                            if resp.clicked() {
                                // nada: chips só removem
                            }
                        }
                        if let Some(i) = remove {
                            doc.remove_filter(i);
                        }
                        let clear = ui.add(
                            egui::Label::new(RichText::new("Limpar tudo").color(p.accent).font(theme::medium(12.5)))
                                .sense(Sense::click()),
                        );
                        if clear.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                            doc.clear_filters();
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let shown = doc.view.len();
                            let text = if doc.is_computing() {
                                "Filtrando…".to_owned()
                            } else {
                                format!("{} de {}", format::int(shown), format::plural(table.rows, "linha", "linhas"))
                            };
                            ui.label(RichText::new(text).color(p.text_muted).font(theme::regular(12.5)));
                        });
                    });
                });
        }

        // Abas das planilhas (Excel/ODS).
        let sheets = doc.info.as_ref().map(|i| (i.sheets.clone(), i.sheet.clone()));
        if let Some((sheets, current)) = sheets
            && sheets.len() > 1
        {
            let mut switch = None;
            egui::Panel::bottom(Id::new(("sheets", doc.id)))
                .exact_size(38.0)
                .show_separator_line(false)
                .frame(Frame::new().fill(p.panel).inner_margin(Margin::symmetric(10, 0)))
                .show(ui, |ui| {
                    let r = ui.max_rect();
                    ui.painter().hline(r.x_range().expand(10.0), r.min.y + 0.5, Stroke::new(1.0, p.border));
                    egui::ScrollArea::horizontal()
                        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                        .show(ui, |ui| {
                            ui.horizontal_centered(|ui| {
                                ui.label(theme::icon_text(ic::SQUARES_FOUR, 15.0).color(p.text_faint));
                                for s in &sheets {
                                    let active = current.as_ref() == Some(s);
                                    if sheet_tab(ui, s, active).clicked() && !active {
                                        switch = Some(s.clone());
                                    }
                                }
                            });
                        });
                });
            if let Some(s) = switch {
                let mut opts = doc.opts.clone();
                opts.sheet = Some(s);
                doc.order.clear();
                doc.hidden.clear();
                doc.reload(opts, ctx);
                return;
            }
        }

        // Grade.
        let view = doc.view.clone();
        let cols = doc.visible_cols();
        let filtered_cols: Vec<usize> = doc.spec.filters.iter().map(|f| f.col).collect();
        let actions = egui::CentralPanel::default()
            .frame(Frame::new().fill(p.bg))
            .show(ui, |ui| {
                let g = GridCtx {
                    id: Id::new(("grid", doc.id)),
                    table: &table,
                    view: &view,
                    cols: &cols,
                    sort: doc.spec.sort,
                    needle: (!doc.needle.is_empty()).then_some(&doc.needle),
                    filtered_cols: &filtered_cols,
                    compact,
                    zebra,
                };
                grid::show(ui, &mut doc.grid, &g)
            })
            .inner;

        for a in actions {
            let doc = &mut self.docs[active];
            match a {
                GridAction::Sort(s) => doc.set_sort(s),
                GridAction::AddFilter(f) => doc.add_filter(f),
                GridAction::ClearFilters => doc.clear_filters(),
                GridAction::HideColumn(c) => {
                    if doc.visible_cols().len() > 1 {
                        doc.set_hidden(c, true);
                        let name = table.columns[c].name.clone();
                        self.toast(ic::EYE_SLASH, format!("Coluna “{name}” oculta"));
                    }
                }
                GridAction::FocusColumnFilter(c) => {
                    self.settings.sidebar = true;
                    let row = doc.grid.cursor.map_or(0, |cur| cur.row);
                    if !doc.view.is_empty() {
                        doc.grid.cursor = Some(CellPos { row, col: c });
                        doc.grid.anchor = doc.grid.cursor;
                    }
                    doc.focus_column_filter = true;
                }
                GridAction::OpenCell { row, col } => {
                    let column = &table.columns[col];
                    let value = column.get(row).to_owned();
                    let trimmed = value.trim_start();
                    let pretty = if trimmed.starts_with('{') || trimmed.starts_with('[') {
                        serde_json::from_str::<serde_json::Value>(&value)
                            .ok()
                            .and_then(|v| serde_json::to_string_pretty(&v).ok())
                    } else {
                        None
                    };
                    let chars = value.chars().count();
                    self.modal = Some(Modal::Cell(CellModal {
                        title: column.name.clone(),
                        subtitle: format!(
                            "Linha {}  ·  Coluna {}  ·  {}  ·  {}",
                            format::int(row + 1),
                            format::column_letter(col),
                            column.kind.label(),
                            format::plural(chars, "caractere", "caracteres")
                        ),
                        show_pretty: pretty.is_some(),
                        pretty,
                        value,
                    }));
                }
                GridAction::Toast(t) => self.toast(ic::CHECK_CIRCLE, t),
            }
        }
    }

    // ------------------------------------------------------------------ janelas

    fn modals(&mut self, ctx: &egui::Context) {
        let p = Palette::of_ctx(ctx);
        let frame = Frame::new()
            .fill(p.elevated)
            .stroke(Stroke::new(1.0, p.border))
            .corner_radius(CornerRadius::same(14))
            .inner_margin(Margin::same(22))
            .shadow(egui::Shadow {
                offset: [0, 16],
                blur: 48,
                spread: 0,
                color: p.shadow,
            });
        let backdrop = if p.dark {
            Color32::from_black_alpha(150)
        } else {
            Color32::from_black_alpha(70)
        };
        let mut close = false;
        let mut reload: Option<(u64, LoadOptions)> = None;
        let mut toast: Option<String> = None;
        match &mut self.modal {
            None => return,
            Some(Modal::Cell(m)) => {
                let resp = egui::Modal::new(Id::new("cell_modal"))
                    .frame(frame)
                    .backdrop_color(backdrop)
                    .show(ctx, |ui| {
                        ui.set_width(620.0f32.min(ctx.content_rect().width() - 80.0));
                        ui.label(RichText::new(&m.title).font(theme::semibold(17.0)).color(p.text));
                        ui.label(RichText::new(&m.subtitle).color(p.text_muted).font(theme::regular(12.0)));
                        ui.add_space(12.0);
                        let shown: &str = if m.show_pretty {
                            m.pretty.as_deref().unwrap_or(&m.value)
                        } else {
                            &m.value
                        };
                        Frame::new()
                            .fill(p.input_bg)
                            .stroke(Stroke::new(1.0, p.border))
                            .corner_radius(CornerRadius::same(10))
                            .inner_margin(Margin::same(12))
                            .show(ui, |ui| {
                                egui::ScrollArea::vertical().max_height(380.0).show(ui, |ui| {
                                    let mut text = shown;
                                    ui.add(
                                        TextEdit::multiline(&mut text)
                                            .font(theme::mono(13.0))
                                            .desired_width(f32::INFINITY)
                                            .frame(Frame::NONE),
                                    );
                                });
                            });
                        ui.add_space(14.0);
                        ui.horizontal(|ui| {
                            if widgets::primary_button_icon(ui, Some(ic::COPY), "Copiar").clicked() {
                                ctx.copy_text(shown.to_owned());
                                toast = Some("Conteúdo copiado".to_owned());
                            }
                            if m.pretty.is_some() {
                                let label = if m.show_pretty { "Ver original" } else { "Formatar JSON" };
                                if widgets::ghost_button_icon(ui, Some(ic::BRACKETS_CURLY), label).clicked() {
                                    m.show_pretty = !m.show_pretty;
                                }
                            }
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if widgets::ghost_button(ui, "Fechar").clicked() {
                                    close = true;
                                }
                            });
                        });
                    });
                if resp.should_close() {
                    close = true;
                }
            }
            Some(Modal::Import(m)) => {
                let doc = self.docs.iter().find(|d| d.id == m.doc_id);
                let Some(doc) = doc else {
                    self.modal = None;
                    return;
                };
                let is_workbook = loader::is_workbook_path(&doc.path)
                    || doc.info.as_ref().is_some_and(|i| i.format == FileFormat::Workbook);
                let detected_delim = doc.info.as_ref().and_then(|i| i.delimiter);
                let detected_enc = doc.info.as_ref().and_then(|i| i.encoding);
                let title = doc.title.clone();
                let resp = egui::Modal::new(Id::new("import_modal"))
                    .frame(frame)
                    .backdrop_color(backdrop)
                    .show(ctx, |ui| {
                        ui.set_width(440.0);
                        ui.label(RichText::new("Opções de importação").font(theme::semibold(17.0)).color(p.text));
                        ui.label(RichText::new(&title).color(p.text_muted).font(theme::regular(12.0)));
                        ui.add_space(16.0);
                        egui::Grid::new("import_grid")
                            .num_columns(2)
                            .spacing(vec2(16.0, 12.0))
                            .show(ui, |ui| {
                                if !is_workbook {
                                    ui.label(RichText::new("Separador").color(p.text_muted));
                                    let auto = match detected_delim {
                                        Some(d) => format!("Automático ({})", delimiter_label(d)),
                                        None => "Automático".to_owned(),
                                    };
                                    let current = match m.delimiter {
                                        None => auto.clone(),
                                        Some(d) => format!("{} ({})", delimiter_label(d), delimiter_symbol(d)),
                                    };
                                    egui::ComboBox::from_id_salt("delim")
                                        .selected_text(current)
                                        .width(270.0)
                                        .show_ui(ui, |ui| {
                                            ui.selectable_value(&mut m.delimiter, None, auto);
                                            for d in *b",;\t|" {
                                                ui.selectable_value(
                                                    &mut m.delimiter,
                                                    Some(d),
                                                    format!("{} ({})", delimiter_label(d), delimiter_symbol(d)),
                                                );
                                            }
                                        });
                                    ui.end_row();

                                    ui.label(RichText::new("Codificação").color(p.text_muted));
                                    let auto = match detected_enc {
                                        Some(e) => format!("Automática ({})", e.split(" (").next().unwrap_or(e)),
                                        None => "Automática".to_owned(),
                                    };
                                    let current = match m.encoding {
                                        None => auto.clone(),
                                        Some(e) => loader::encoding_label(e).to_owned(),
                                    };
                                    egui::ComboBox::from_id_salt("enc")
                                        .selected_text(current)
                                        .width(270.0)
                                        .show_ui(ui, |ui| {
                                            ui.selectable_value(&mut m.encoding, None, auto);
                                            for e in [
                                                encoding_rs::UTF_8,
                                                encoding_rs::WINDOWS_1252,
                                                encoding_rs::ISO_8859_15,
                                                encoding_rs::UTF_16LE,
                                                encoding_rs::UTF_16BE,
                                            ] {
                                                ui.selectable_value(&mut m.encoding, Some(e), loader::encoding_label(e));
                                            }
                                        });
                                    ui.end_row();
                                }
                                ui.label(RichText::new("Cabeçalho").color(p.text_muted));
                                ui.checkbox(&mut m.has_header, "Primeira linha tem os nomes das colunas");
                                ui.end_row();
                            });
                        ui.add_space(20.0);
                        ui.horizontal(|ui| {
                            if widgets::primary_button(ui, "Aplicar e recarregar").clicked() {
                                reload = Some((
                                    m.doc_id,
                                    LoadOptions {
                                        has_header: m.has_header,
                                        delimiter: m.delimiter,
                                        encoding: m.encoding,
                                        sheet: None,
                                    },
                                ));
                                close = true;
                            }
                            if widgets::ghost_button(ui, "Cancelar").clicked() {
                                close = true;
                            }
                        });
                    });
                if resp.should_close() {
                    close = true;
                }
            }
            Some(Modal::Shortcuts) => {
                let resp = egui::Modal::new(Id::new("shortcuts_modal"))
                    .frame(frame)
                    .backdrop_color(backdrop)
                    .show(ctx, |ui| {
                        ui.set_width(520.0);
                        ui.label(RichText::new("Atalhos de teclado").font(theme::semibold(17.0)).color(p.text));
                        ui.add_space(14.0);
                        let rows: &[(&[&str], &str)] = &[
                            (&["Ctrl", "O"], "Abrir arquivo"),
                            (&["Ctrl", "F"], "Buscar em todas as colunas"),
                            (&["Ctrl", "C"], "Copiar seleção (cola no Excel)"),
                            (&["Ctrl", "A"], "Selecionar tudo"),
                            (&["Shift", "Setas"], "Estender a seleção"),
                            (&["Ctrl", "Setas"], "Ir até a borda"),
                            (&["PgUp", "PgDn"], "Rolar uma página"),
                            (&["Enter"], "Ver conteúdo completo da célula"),
                            (&["Ctrl", "E"], "Exportar visão atual"),
                            (&["Ctrl", "B"], "Mostrar/ocultar painel de colunas"),
                            (&["Ctrl", "Tab"], "Próxima aba"),
                            (&["Ctrl", "W"], "Fechar aba"),
                            (&["Ctrl", "R"], "Recarregar arquivo"),
                            (&["Ctrl", "+ / −"], "Zoom"),
                        ];
                        egui::Grid::new("shortcuts_grid")
                            .num_columns(2)
                            .spacing(vec2(20.0, 9.0))
                            .show(ui, |ui| {
                                for (keys, desc) in rows {
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = 4.0;
                                        for k in keys.iter() {
                                            widgets::kbd(ui, k);
                                        }
                                    });
                                    ui.label(RichText::new(*desc).color(p.text));
                                    ui.end_row();
                                }
                            });
                        ui.add_space(12.0);
                        ui.label(
                            RichText::new("Dica: clique no cabeçalho para ordenar e use o botão direito nas células para filtrar.")
                                .color(p.text_faint)
                                .font(theme::regular(12.0)),
                        );
                        ui.add_space(14.0);
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if widgets::ghost_button(ui, "Fechar").clicked() {
                                close = true;
                            }
                        });
                    });
                if resp.should_close() {
                    close = true;
                }
            }
        }
        if let Some(t) = toast {
            self.toast(ic::CHECK_CIRCLE, t);
        }
        if close {
            self.modal = None;
        }
        if let Some((id, opts)) = reload
            && let Some(doc) = self.docs.iter_mut().find(|d| d.id == id)
        {
            let opts = LoadOptions {
                sheet: doc.opts.sheet.clone(),
                ..opts
            };
            doc.reload(opts, ctx);
        }
    }

    fn paint_toasts(&mut self, ctx: &egui::Context) {
        const LIFE: f32 = 2.6;
        self.toasts.retain(|t| t.created.elapsed().as_secs_f32() < LIFE);
        if self.toasts.is_empty() {
            return;
        }
        let p = Palette::of_ctx(ctx);
        egui::Area::new(Id::new("toasts"))
            .order(Order::Tooltip)
            .anchor(Align2::CENTER_BOTTOM, vec2(0.0, -48.0))
            .interactable(false)
            .show(ctx, |ui| {
                for t in &self.toasts {
                    let age = t.created.elapsed().as_secs_f32();
                    let alpha = (age / 0.15).min(1.0) * ((LIFE - age) / 0.4).clamp(0.0, 1.0);
                    Frame::new()
                        .fill(p.elevated)
                        .stroke(Stroke::new(1.0, p.border_strong))
                        .corner_radius(CornerRadius::same(20))
                        .inner_margin(Margin::symmetric(16, 9))
                        .shadow(egui::Shadow {
                            offset: [0, 6],
                            blur: 20,
                            spread: 0,
                            color: p.shadow,
                        })
                        .multiply_with_opacity(alpha)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let color = if t.error { p.danger } else { p.accent };
                                ui.label(theme::icon_text(t.icon, 15.0).color(color.gamma_multiply(alpha)).size(15.0));
                                ui.label(
                                    RichText::new(&t.text)
                                        .font(theme::medium(13.0))
                                        .color(p.text.gamma_multiply(alpha)),
                                );
                            });
                        });
                    ui.add_space(6.0);
                }
            });
        ctx.request_repaint_after(Duration::from_millis(40));
    }

    fn drop_overlay(&self, ctx: &egui::Context) {
        let hovering = ctx.input(|i| !i.raw.hovered_files.is_empty());
        if !hovering {
            return;
        }
        let p = Palette::of_ctx(ctx);
        let painter = ctx.layer_painter(LayerId::new(Order::Foreground, Id::new("drop_overlay")));
        let rect = ctx.content_rect();
        // A mistura é feita em espaço linear: precisa de bastante opacidade
        // para o conteúdo de trás virar só um "fantasma".
        painter.rect_filled(rect, 0.0, p.bg.gamma_multiply(0.985));
        let inner = rect.shrink(28.0);
        painter.rect(
            inner,
            CornerRadius::same(22),
            p.accent_faint,
            Stroke::new(2.0, p.accent),
            StrokeKind::Inside,
        );
        painter.text(
            inner.center() - vec2(0.0, 30.0),
            Align2::CENTER_CENTER,
            ic::DOWNLOAD_SIMPLE,
            theme::icon(44.0),
            p.accent,
        );
        painter.text(
            inner.center() + vec2(0.0, 22.0),
            Align2::CENTER_CENTER,
            "Solte para abrir",
            theme::semibold(20.0),
            p.text,
        );
    }
}

/// Item de menu liga/desliga (com ícone de interruptor).
fn toggle_item(ui: &mut Ui, label: &str, on: bool) -> bool {
    let p = Palette::of(ui);
    let icon = if on { ic::TOGGLE_RIGHT } else { ic::TOGGLE_LEFT };
    let color = if on { p.accent } else { p.text_faint };
    ui.add(
        egui::Button::new((theme::icon_text(icon, 17.0).color(color), label.to_owned()))
            .gap(8.0)
            .frame_when_inactive(false),
    )
    .clicked()
}

/// Aba de arquivo na barra superior. Devolve (resposta, clicou no ×).
fn tab(ui: &mut Ui, doc: &Document, active: bool) -> (egui::Response, bool) {
    let p = Palette::of(ui);
    let title: String = {
        let t: String = doc.title.chars().take(30).collect();
        if t.chars().count() < doc.title.chars().count() { format!("{t}…") } else { t }
    };
    let font = if active { theme::medium(13.0) } else { theme::regular(13.0) };
    let color = if active { p.text } else { p.text_muted };
    let galley = ui.painter().layout_no_wrap(title, font, color);
    let w = 12.0 + 16.0 + 8.0 + galley.size().x + 8.0 + 20.0 + 6.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 34.0), Sense::click());
    let close_rect = Rect::from_center_size(pos2(rect.max.x - 16.0, rect.center().y), vec2(20.0, 20.0));
    let close = ui.interact(close_rect, resp.id.with("close"), Sense::click());
    let hovered = resp.hovered() || close.hovered();

    if active {
        ui.painter().rect(rect, CornerRadius::same(9), p.surface, Stroke::new(1.0, p.border), StrokeKind::Inside);
        let bar = Rect::from_min_max(pos2(rect.min.x + 12.0, rect.max.y - 2.5), pos2(rect.max.x - 12.0, rect.max.y - 0.5));
        ui.painter().rect_filled(bar, CornerRadius::same(1), p.accent);
    } else if hovered {
        ui.painter().rect_filled(rect, CornerRadius::same(9), p.surface_hover.gamma_multiply(0.7));
    }

    let icon_pos = pos2(rect.min.x + 12.0, rect.center().y);
    match &doc.state {
        DocState::Loading { .. } => {
            let r = Rect::from_min_size(icon_pos - vec2(0.0, 7.0), vec2(14.0, 14.0));
            egui::Spinner::new().color(p.accent).paint_at(ui, r);
        }
        DocState::Failed(_) => {
            ui.painter().text(icon_pos, Align2::LEFT_CENTER, ic::WARNING, theme::icon(15.0), p.danger);
        }
        DocState::Ready => {
            ui.painter().text(
                icon_pos,
                Align2::LEFT_CENTER,
                welcome::file_icon(&doc.path),
                theme::icon(16.0),
                if active { p.accent } else { p.text_faint },
            );
        }
    }
    ui.painter().galley(
        pos2(rect.min.x + 36.0, rect.center().y - galley.size().y / 2.0),
        galley,
        color,
    );
    if active || hovered {
        if close.hovered() {
            ui.painter().circle_filled(close_rect.center(), 9.0, p.border_strong);
        }
        ui.painter().text(
            close_rect.center(),
            Align2::CENTER_CENTER,
            ic::X,
            theme::icon(12.0),
            if close.hovered() { p.text } else { p.text_muted },
        );
    }
    let clicked_close = close.on_hover_text("Fechar (Ctrl+W)").clicked();
    let resp = resp.on_hover_text(doc.path.display().to_string());
    (resp, clicked_close)
}

fn sheet_tab(ui: &mut Ui, name: &str, active: bool) -> egui::Response {
    let p = Palette::of(ui);
    let font = if active { theme::medium(12.5) } else { theme::regular(12.5) };
    let galley = ui
        .painter()
        .layout_no_wrap(name.to_owned(), font, if active { p.accent } else { p.text_muted });
    let (rect, resp) = ui.allocate_exact_size(vec2(galley.size().x + 24.0, 26.0), Sense::click());
    if active {
        ui.painter().rect_filled(rect, CornerRadius::same(13), p.accent_soft);
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, CornerRadius::same(13), p.surface_hover);
    }
    let color = if active { p.accent } else { p.text_muted };
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, color);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn search_box(ui: &mut Ui, doc: &mut Document, focus: bool) {
    let p = Palette::of(ui);
    let id = Id::new(("search", doc.id));
    let out = TextEdit::singleline(&mut doc.search_input)
        .id(id)
        .hint_text(RichText::new("Buscar em todas as colunas…").color(p.text_faint))
        .prefix(theme::icon_text(ic::MAGNIFYING_GLASS, 15.0).color(p.text_faint))
        .margin(Margin::symmetric(10, 7))
        .desired_width(300.0)
        .show(ui);
    let resp = out.response.response;
    if focus {
        resp.request_focus();
    }
    // O TextEdit já perde o foco no Esc antes de chegarmos aqui.
    if (resp.has_focus() || resp.lost_focus()) && ui.input(|i| i.key_pressed(Key::Escape)) {
        doc.search_input.clear();
        resp.surrender_focus();
    }
    // Enter: vai para a grade (primeiro resultado).
    if resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
        ui.memory_mut(|m| m.request_focus(Id::new(("grid", doc.id)).with("body")));
    }
    let r = resp.rect;
    if doc.search_input.is_empty() {
        if !resp.has_focus() {
            ui.painter().text(
                pos2(r.max.x - 12.0, r.center().y),
                Align2::RIGHT_CENTER,
                "Ctrl+F",
                theme::regular(11.0),
                p.text_faint,
            );
        }
    } else {
        let x_rect = Rect::from_center_size(pos2(r.max.x - 15.0, r.center().y), vec2(20.0, 20.0));
        let x = ui.interact(x_rect, id.with("clear"), Sense::click());
        ui.painter().text(
            x_rect.center(),
            Align2::CENTER_CENTER,
            ic::X_CIRCLE,
            theme::icon(15.0),
            if x.hovered() { p.text } else { p.text_faint },
        );
        if x.on_hover_text("Limpar busca (Esc)").clicked() {
            doc.search_input.clear();
        }
    }
}

impl eframe::App for App {
    #[cfg(feature = "devshot")]
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        if let Some(d) = &mut self.devshot {
            d.hook(ctx, raw_input);
        }
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        #[cfg(feature = "devshot")]
        if let Some(d) = &mut self.devshot {
            d.receive(&ctx);
        }
        self.handle_dropped_files(&ctx);
        self.poll_dialogs(&ctx);
        let mut loaded = Vec::new();
        for doc in &mut self.docs {
            doc.poll(&ctx);
            if std::mem::take(&mut doc.just_loaded) {
                loaded.push(doc.path.clone());
            }
        }
        for path in loaded {
            self.remember_recent(&path);
        }
        if self.modal.is_none() {
            self.handle_shortcuts(&ctx);
        }
        self.update_title(&ctx);

        self.top_bar(ui);
        if self.ready_doc().is_some() {
            self.status_bar(ui);
            if self.settings.sidebar {
                self.side_panel(ui);
            }
        }
        self.central(ui);
        self.modals(&ctx);
        self.paint_toasts(&ctx);
        self.drop_overlay(&ctx);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, &self.settings);
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        let p = if self.settings.dark { &theme::DARK } else { &theme::LIGHT };
        p.bg.to_normalized_gamma_f32()
    }
}
