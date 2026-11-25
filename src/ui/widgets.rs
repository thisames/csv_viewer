//! Pequenos componentes visuais reutilizados pela interface.

use egui::epaint::text::{LayoutJob, TextFormat, TextWrapping};
use egui::{
    Align, Align2, Color32, CornerRadius, FontId, Rect, Response, Sense, Stroke, StrokeKind, Ui,
    Vec2, pos2, vec2,
};
use egui_phosphor::regular as ic;

use crate::theme::{self, Palette};

/// Botão quadrado só com ícone, sem moldura até passar o mouse.
pub fn icon_button(ui: &mut Ui, icon: &str, tooltip: &str, active: bool) -> Response {
    let p = Palette::of(ui);
    let size = vec2(32.0, 32.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let hovered = resp.hovered();
    let bg = if active {
        p.accent_soft
    } else if hovered {
        p.surface_hover
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, CornerRadius::same(8), bg);
    let color = if active {
        p.accent
    } else if hovered {
        p.text
    } else {
        p.text_muted
    };
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        icon,
        theme::icon(17.0),
        color,
    );
    resp.on_hover_text(tooltip)
}

/// Botão de destaque (cor de acento).
pub fn primary_button(ui: &mut Ui, text: &str) -> Response {
    primary_button_icon(ui, None, text)
}

pub fn primary_button_icon(ui: &mut Ui, icon: Option<&str>, text: &str) -> Response {
    let p = Palette::of(ui);
    let font = theme::semibold(13.5);
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), font, p.on_accent);
    let icon_w = if icon.is_some() { 24.0 } else { 0.0 };
    let size = vec2(galley.size().x + 32.0 + icon_w, 36.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let fill = if resp.is_pointer_button_down_on() {
        p.accent.gamma_multiply(0.85)
    } else if resp.hovered() {
        p.accent_hover
    } else {
        p.accent
    };
    ui.painter().rect_filled(rect, CornerRadius::same(9), fill);
    let text_pos = rect.center() - galley.size() / 2.0 + vec2(icon_w / 2.0, 0.0);
    if let Some(icon) = icon {
        ui.painter().text(
            pos2(text_pos.x - 8.0, rect.center().y),
            Align2::RIGHT_CENTER,
            icon,
            theme::icon(16.0),
            p.on_accent,
        );
    }
    ui.painter().galley(text_pos, galley, p.on_accent);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Botão secundário com contorno.
pub fn ghost_button(ui: &mut Ui, text: &str) -> Response {
    ghost_button_icon(ui, None, text)
}

pub fn ghost_button_icon(ui: &mut Ui, icon: Option<&str>, text: &str) -> Response {
    let p = Palette::of(ui);
    let font = theme::medium(13.0);
    let galley = ui.painter().layout_no_wrap(text.to_owned(), font, p.text);
    let icon_w = if icon.is_some() { 22.0 } else { 0.0 };
    let size = vec2(galley.size().x + 26.0 + icon_w, 34.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let fill = if resp.hovered() {
        p.surface_hover
    } else {
        p.surface
    };
    ui.painter().rect(
        rect,
        CornerRadius::same(9),
        fill,
        Stroke::new(1.0, p.border),
        StrokeKind::Inside,
    );
    let text_pos = rect.center() - galley.size() / 2.0 + vec2(icon_w / 2.0, 0.0);
    if let Some(icon) = icon {
        ui.painter().text(
            pos2(text_pos.x - 7.0, rect.center().y),
            Align2::RIGHT_CENTER,
            icon,
            theme::icon(15.0),
            p.text_muted,
        );
    }
    ui.painter().galley(text_pos, galley, p.text);
    resp
}

/// Ícone + texto numa linha.
pub fn icon_label(ui: &mut Ui, icon: &str, text: &str, font: FontId, color: Color32) -> Response {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        ui.label(theme::icon_text(icon, font.size + 1.0).color(color));
        ui.label(egui::RichText::new(text).font(font).color(color));
    })
    .response
}

/// Pílula pequena (rótulo de tipo, contadores).
pub fn badge(ui: &mut Ui, text: &str, fg: Color32, bg: Color32) -> Response {
    let font = theme::medium(11.0);
    let galley = ui.painter().layout_no_wrap(text.to_owned(), font, fg);
    let size = vec2(galley.size().x + 14.0, 20.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::same(10), bg);
    ui.painter()
        .galley(rect.center() - galley.size() / 2.0, galley, fg);
    resp
}

/// Chip de filtro com botão de remover. Devolve (resposta do chip, clicou no ×).
pub fn filter_chip(ui: &mut Ui, icon: &str, text: &str) -> (Response, bool) {
    let p = Palette::of(ui);
    let font = theme::medium(12.5);
    let job = LayoutJob {
        wrap: TextWrapping::truncate_at_width(260.0),
        ..LayoutJob::single_section(
            text.to_owned(),
            TextFormat {
                font_id: font,
                color: p.text,
                valign: Align::Center,
                ..Default::default()
            },
        )
    };
    let galley = ui.painter().layout_job(job);
    let w = 10.0 + 16.0 + 6.0 + galley.size().x + 6.0 + 20.0 + 4.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 28.0), Sense::hover());
    let close_rect = Rect::from_center_size(pos2(rect.max.x - 14.0, rect.center().y), vec2(20.0, 20.0));
    let close = ui.interact(close_rect, resp.id.with("x"), Sense::click());
    ui.painter().rect(
        rect,
        CornerRadius::same(14),
        p.accent_faint,
        Stroke::new(1.0, p.accent_soft),
        StrokeKind::Inside,
    );
    ui.painter().text(
        pos2(rect.min.x + 10.0, rect.center().y),
        Align2::LEFT_CENTER,
        icon,
        theme::icon(14.0),
        p.accent,
    );
    ui.painter().galley(
        pos2(rect.min.x + 32.0, rect.center().y - galley.size().y / 2.0),
        galley,
        p.text,
    );
    if close.hovered() {
        ui.painter()
            .circle_filled(close_rect.center(), 9.0, p.accent_soft);
    }
    ui.painter().text(
        close_rect.center(),
        Align2::CENTER_CENTER,
        ic::X,
        theme::icon(12.0),
        if close.hovered() { p.text } else { p.text_muted },
    );
    let clicked = close.on_hover_text("Remover filtro").clicked();
    (resp, clicked)
}

/// Título de seção em letras pequenas.
pub fn section_label(ui: &mut Ui, text: &str) {
    let p = Palette::of(ui);
    ui.label(
        egui::RichText::new(text.to_uppercase())
            .font(theme::semibold(10.5))
            .color(p.text_faint)
            .extra_letter_spacing(0.6),
    );
}

/// Cartão com um número grande e um rótulo.
pub fn stat_tile(ui: &mut Ui, width: f32, label: &str, value: &str, hint: Option<&str>) -> Response {
    let p = Palette::of(ui);
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 54.0), Sense::hover());
    ui.painter().rect(
        rect,
        CornerRadius::same(9),
        p.surface,
        Stroke::new(1.0, p.border),
        StrokeKind::Inside,
    );
    ui.painter().text(
        pos2(rect.min.x + 11.0, rect.min.y + 15.0),
        Align2::LEFT_CENTER,
        label,
        theme::regular(11.0),
        p.text_faint,
    );
    let job = LayoutJob {
        wrap: TextWrapping::truncate_at_width(width - 20.0),
        ..LayoutJob::single_section(
            value.to_owned(),
            TextFormat {
                font_id: theme::semibold(14.5),
                color: p.text,
                valign: Align::Center,
                ..Default::default()
            },
        )
    };
    let galley = ui.painter().layout_job(job);
    let truncated = galley.size().x >= width - 21.0;
    ui.painter()
        .galley(pos2(rect.min.x + 11.0, rect.min.y + 26.0), galley, p.text);
    match (hint, truncated) {
        (Some(h), _) => resp.on_hover_text(h),
        (None, true) => resp.on_hover_text(value),
        _ => resp,
    }
}

/// Barra fina de progresso/proporção.
pub fn ratio_bar(ui: &mut Ui, width: f32, fraction: f32, color: Color32) {
    let p = Palette::of(ui);
    let (rect, _) = ui.allocate_exact_size(vec2(width, 6.0), Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::same(3), p.surface_hover);
    let mut fill = rect;
    fill.set_width((rect.width() * fraction.clamp(0.0, 1.0)).max(if fraction > 0.0 { 3.0 } else { 0.0 }));
    ui.painter().rect_filled(fill, CornerRadius::same(3), color);
}

/// Tecla de atalho desenhada como um "botão de teclado".
pub fn kbd(ui: &mut Ui, text: &str) {
    let p = Palette::of(ui);
    let font = theme::medium(11.5);
    let galley = ui.painter().layout_no_wrap(text.to_owned(), font, p.text_muted);
    let size = vec2(galley.size().x + 12.0, 22.0);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter().rect(
        rect,
        CornerRadius::same(5),
        p.surface,
        Stroke::new(1.0, p.border_strong),
        StrokeKind::Inside,
    );
    ui.painter().hline(
        rect.x_range().shrink(3.0),
        rect.max.y - 1.0,
        Stroke::new(1.0, p.border_strong),
    );
    ui.painter()
        .galley(rect.center() - galley.size() / 2.0, galley, p.text_muted);
}

/// Logotipo (textura gerada em `icon.rs`).
pub fn logo(ui: &mut Ui, texture: &egui::TextureHandle, size: f32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    ui.painter().image(
        texture.id(),
        rect,
        Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
        Color32::WHITE,
    );
    resp
}

/// Mini histograma (lista de colunas).
pub fn sparkline(ui: &mut Ui, bins: &[u32], size: Vec2, color: Color32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    let max = bins.iter().copied().max().unwrap_or(0).max(1) as f32;
    let n = bins.len().max(1) as f32;
    let bw = rect.width() / n;
    for (i, &b) in bins.iter().enumerate() {
        if b == 0 {
            continue;
        }
        let h = (b as f32 / max * rect.height()).max(1.5);
        let x = rect.min.x + i as f32 * bw;
        let bar = Rect::from_min_max(pos2(x + 0.5, rect.max.y - h), pos2(x + bw - 0.5, rect.max.y));
        ui.painter().rect_filled(bar, CornerRadius::same(1), color);
    }
    resp
}

/// Texto truncado em uma linha, desenhado num retângulo.
pub fn paint_truncated(
    painter: &egui::Painter,
    pos: egui::Pos2,
    text: &str,
    font: FontId,
    color: Color32,
    max_w: f32,
) -> Rect {
    let job = LayoutJob {
        wrap: TextWrapping::truncate_at_width(max_w.max(4.0)),
        ..LayoutJob::single_section(
            text.to_owned(),
            TextFormat {
                font_id: font,
                color,
                valign: Align::Center,
                ..Default::default()
            },
        )
    };
    let galley = painter.layout_job(job);
    let rect = Rect::from_min_size(pos2(pos.x, pos.y - galley.size().y / 2.0), galley.size());
    painter.galley(rect.min, galley, color);
    rect
}
