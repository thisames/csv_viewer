//! Telas sem tabela: boas-vindas, carregando e erro.

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::Instant;

use egui::{
    Align2, Color32, CornerRadius, Rect, RichText, Sense, Shape, Stroke, StrokeKind, Ui, pos2, vec2,
};
use egui_phosphor::regular as ic;

use crate::data::loader::{self, Progress};
use crate::format;
use crate::theme::{self, Palette};
use crate::ui::widgets;

pub enum WelcomeAction {
    Open,
    OpenRecent(PathBuf),
    ForgetRecent(PathBuf),
}

pub fn file_icon(path: &Path) -> &'static str {
    if loader::is_workbook_path(path) {
        ic::FILE_XLS
    } else {
        match path
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("csv") => ic::FILE_CSV,
            _ => ic::FILE_TEXT,
        }
    }
}

fn home_relative(path: &Path) -> String {
    let parent = path.parent().unwrap_or(path);
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from)
        && let Ok(rest) = parent.strip_prefix(&home)
    {
        return if rest.as_os_str().is_empty() {
            "~".to_owned()
        } else {
            format!("~/{}", rest.display())
        };
    }
    parent.display().to_string()
}

/// Retângulo tracejado arredondado (área de soltar arquivos).
fn dashed_round_rect(ui: &Ui, rect: Rect, radius: f32, stroke: Stroke) {
    let r = radius;
    let mut pts = Vec::new();
    let corner = |cx: f32, cy: f32, a0: f32, pts: &mut Vec<egui::Pos2>| {
        for i in 0..=8 {
            let a = a0 + i as f32 / 8.0 * std::f32::consts::FRAC_PI_2;
            pts.push(pos2(cx + r * a.cos(), cy + r * a.sin()));
        }
    };
    use std::f32::consts::PI;
    corner(rect.max.x - r, rect.min.y + r, -PI / 2.0, &mut pts);
    corner(rect.max.x - r, rect.max.y - r, 0.0, &mut pts);
    corner(rect.min.x + r, rect.max.y - r, PI / 2.0, &mut pts);
    corner(rect.min.x + r, rect.min.y + r, PI, &mut pts);
    pts.push(pts[0]);
    ui.painter()
        .extend(Shape::dashed_line(&pts, stroke, 7.0, 5.0));
}

pub fn show(ui: &mut Ui, recent: &[PathBuf], logo: &egui::TextureHandle) -> Option<WelcomeAction> {
    let p = Palette::of(ui);
    let mut action = None;
    let avail = ui.available_rect_before_wrap();
    let width = 520.0f32.min(avail.width() - 48.0);
    let recent: Vec<&PathBuf> = recent.iter().take(6).collect();
    let content_h = 360.0 + if recent.is_empty() { 0.0 } else { 40.0 + recent.len() as f32 * 46.0 };
    let top = avail.min.y + ((avail.height() - content_h) / 2.0).max(24.0);

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let full = ui.max_rect();
            let col = Rect::from_min_size(pos2(full.center().x - width / 2.0, top), vec2(width, content_h));
            let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(col));
            let ui = &mut ui;

            ui.vertical_centered(|ui| {
                widgets::logo(ui, logo, 64.0);
                ui.add_space(14.0);
                ui.label(RichText::new("Abra uma planilha").font(theme::semibold(26.0)).color(p.text));
                ui.add_space(2.0);
                ui.label(
                    RichText::new("CSV, TSV, TXT, Excel (.xlsx, .xls) e OpenDocument (.ods)")
                        .color(p.text_muted)
                        .font(theme::regular(13.5)),
                );
            });
            ui.add_space(22.0);

            // Área de soltar.
            let (zone, zone_resp) = ui.allocate_exact_size(vec2(width, 150.0), Sense::click());
            let hovered = zone_resp.hovered();
            ui.painter().rect_filled(
                zone,
                CornerRadius::same(16),
                if hovered { p.accent_faint } else { p.panel },
            );
            dashed_round_rect(
                ui,
                zone.shrink(0.5),
                16.0,
                Stroke::new(1.5, if hovered { p.accent } else { p.border_strong }),
            );
            ui.painter().text(
                zone.center() - vec2(0.0, 36.0),
                Align2::CENTER_CENTER,
                ic::UPLOAD_SIMPLE,
                theme::icon(28.0),
                if hovered { p.accent } else { p.text_muted },
            );
            ui.painter().text(
                zone.center() - vec2(0.0, 2.0),
                Align2::CENTER_CENTER,
                "Arraste um arquivo para cá",
                theme::medium(15.0),
                p.text,
            );
            ui.painter().text(
                zone.center() + vec2(0.0, 22.0),
                Align2::CENTER_CENTER,
                "ou clique para escolher  ·  Ctrl+O",
                theme::regular(12.5),
                p.text_muted,
            );
            if zone_resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                action = Some(WelcomeAction::Open);
            }

            if !recent.is_empty() {
                ui.add_space(26.0);
                widgets::section_label(ui, "Abertos recentemente");
                ui.add_space(6.0);
                for path in recent {
                    let (rect, resp) = ui.allocate_exact_size(vec2(width, 42.0), Sense::click());
                    let forget_rect = Rect::from_center_size(pos2(rect.max.x - 20.0, rect.center().y), vec2(24.0, 24.0));
                    let forget = ui.interact(forget_rect, resp.id.with("forget"), Sense::click());
                    let hov = resp.hovered() || forget.hovered();
                    if hov {
                        ui.painter().rect_filled(rect, CornerRadius::same(9), p.surface_hover);
                    }
                    ui.painter().text(
                        pos2(rect.min.x + 14.0, rect.center().y),
                        Align2::LEFT_CENTER,
                        file_icon(path),
                        theme::icon(20.0),
                        p.accent,
                    );
                    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    widgets::paint_truncated(
                        ui.painter(),
                        pos2(rect.min.x + 44.0, rect.center().y - 8.0),
                        &name,
                        theme::medium(13.5),
                        p.text,
                        width - 100.0,
                    );
                    widgets::paint_truncated(
                        ui.painter(),
                        pos2(rect.min.x + 44.0, rect.center().y + 9.0),
                        &home_relative(path),
                        theme::regular(11.5),
                        p.text_faint,
                        width - 100.0,
                    );
                    if hov {
                        if forget.hovered() {
                            ui.painter().circle_filled(forget_rect.center(), 11.0, p.border);
                        }
                        ui.painter().text(
                            forget_rect.center(),
                            Align2::CENTER_CENTER,
                            ic::X,
                            theme::icon(13.0),
                            p.text_muted,
                        );
                    }
                    if forget.on_hover_text("Remover da lista").clicked() {
                        action = Some(WelcomeAction::ForgetRecent(path.clone()));
                    } else if resp.on_hover_text(path.display().to_string()).clicked() {
                        action = Some(WelcomeAction::OpenRecent(path.clone()));
                    }
                }
            }
        });
    action
}

/// Barra de progresso com faixa animada.
fn progress_bar(ui: &mut Ui, width: f32, fraction: Option<f32>) {
    let p = Palette::of(ui);
    let (rect, _) = ui.allocate_exact_size(vec2(width, 6.0), Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::same(3), p.surface_hover);
    match fraction {
        Some(f) => {
            let mut fill = rect;
            fill.set_width(rect.width() * f.clamp(0.02, 1.0));
            ui.painter().rect_filled(fill, CornerRadius::same(3), p.accent);
        }
        None => {
            let t = (ui.input(|i| i.time) * 0.8).fract() as f32;
            let seg = rect.width() * 0.3;
            let x = rect.min.x - seg + t * (rect.width() + seg);
            let fill = Rect::from_min_max(pos2(x.max(rect.min.x), rect.min.y), pos2((x + seg).min(rect.max.x), rect.max.y));
            ui.painter().rect_filled(fill, CornerRadius::same(3), p.accent);
        }
    }
    ui.ctx().request_repaint();
}

fn centered_card(ui: &mut Ui, width: f32, height: f32, add: impl FnOnce(&mut Ui)) {
    let p = Palette::of(ui);
    let avail = ui.available_rect_before_wrap();
    let rect = Rect::from_center_size(avail.center(), vec2(width.min(avail.width() - 32.0), height));
    ui.painter().rect(
        rect,
        CornerRadius::same(16),
        p.panel,
        Stroke::new(1.0, p.border),
        StrokeKind::Inside,
    );
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink(28.0)));
    add(&mut child);
}

/// Devolve `true` se o usuário cancelou.
pub fn loading(ui: &mut Ui, title: &str, path: &Path, progress: &Progress, started: Instant) -> bool {
    let p = Palette::of(ui);
    let mut cancel = false;
    // Evita "piscar" a tela de carregamento em arquivos pequenos.
    if started.elapsed().as_millis() < 120 {
        ui.ctx().request_repaint();
        return false;
    }
    centered_card(ui, 440.0, 168.0, |ui| {
        ui.horizontal(|ui| {
            ui.label(theme::icon_text(file_icon(path), 26.0).color(p.accent));
            ui.vertical(|ui| {
                ui.label(RichText::new(title).font(theme::semibold(15.0)).color(p.text));
                let size = progress.total.load(Ordering::Relaxed);
                ui.label(RichText::new(format::bytes(size)).color(p.text_faint).font(theme::regular(12.0)));
            });
        });
        ui.add_space(20.0);
        let analyzing = progress.stage.load(Ordering::Relaxed) == loader::STAGE_ANALYZING;
        let frac = if analyzing { None } else { progress.fraction() };
        progress_bar(ui, ui.available_width(), frac);
        ui.add_space(10.0);
        let rows = progress.rows.load(Ordering::Relaxed) as usize;
        let status = if analyzing {
            format!("Analisando colunas…  {} linhas", format::int(rows))
        } else {
            match frac {
                Some(f) => format!("Lendo…  {}%  ·  {} linhas", (f * 100.0).round() as u32, format::int(rows)),
                None => "Lendo…".to_owned(),
            }
        };
        ui.horizontal(|ui| {
            ui.label(RichText::new(status).color(p.text_muted).font(theme::regular(12.5)));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Cancelar").clicked() {
                    cancel = true;
                }
            });
        });
    });
    cancel
}

pub enum ErrorAction {
    Retry,
    Options,
    Close,
}

pub fn error(ui: &mut Ui, title: &str, message: &str) -> Option<ErrorAction> {
    let p = Palette::of(ui);
    let mut action = None;
    centered_card(ui, 480.0, 196.0, |ui| {
        ui.horizontal(|ui| {
            ui.label(theme::icon_text(ic::WARNING, 26.0).color(p.danger));
            ui.vertical(|ui| {
                ui.label(RichText::new("Não foi possível abrir").font(theme::semibold(15.0)).color(p.text));
                ui.label(RichText::new(title).color(p.text_faint).font(theme::regular(12.0)));
            });
        });
        ui.add_space(14.0);
        egui::ScrollArea::vertical().max_height(70.0).show(ui, |ui| {
            ui.label(RichText::new(message).color(p.text_muted));
        });
        ui.add_space(14.0);
        ui.horizontal(|ui| {
            if widgets::primary_button(ui, "Tentar novamente").clicked() {
                action = Some(ErrorAction::Retry);
            }
            if widgets::ghost_button(ui, "Opções de importação").clicked() {
                action = Some(ErrorAction::Options);
            }
            if widgets::ghost_button(ui, "Fechar").clicked() {
                action = Some(ErrorAction::Close);
            }
        });
    });
    let _ = Color32::TRANSPARENT;
    action
}
