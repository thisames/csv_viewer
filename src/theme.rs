//! Aparência: paleta de cores (escura e clara), fontes e estilo do egui.

use std::sync::Arc;

use egui::epaint::text::VariationCoords;
use egui::style::{ScrollStyle, Selection, WidgetVisuals, Widgets};
use egui::{
    Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, FontTweak, Margin,
    Shadow, Stroke, TextStyle, Theme, Ui, Visuals, vec2,
};

use crate::data::table::ColumnKind;

/// Famílias extras (Inter com outros pesos) e ícones.
pub const MEDIUM: &str = "medium";
pub const SEMIBOLD: &str = "semibold";
pub const ICONS: &str = "icons";

/// Fonte de ícones (Phosphor). Use sempre esta para desenhar ícones.
pub fn icon(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(ICONS.into()))
}

pub fn icon_text(glyph: &str, size: f32) -> egui::RichText {
    egui::RichText::new(glyph).font(icon(size))
}

pub fn medium(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(MEDIUM.into()))
}

pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(SEMIBOLD.into()))
}

pub fn regular(size: f32) -> FontId {
    FontId::proportional(size)
}

pub fn mono(size: f32) -> FontId {
    FontId::monospace(size)
}

#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub dark: bool,
    /// Fundo da grade.
    pub bg: Color32,
    /// Barras e painel lateral.
    pub panel: Color32,
    /// Cabeçalho da grade, cartões.
    pub surface: Color32,
    pub surface_hover: Color32,
    /// Menus e janelas flutuantes.
    pub elevated: Color32,
    pub input_bg: Color32,
    pub border: Color32,
    pub border_strong: Color32,
    pub grid_line: Color32,
    pub text: Color32,
    pub text_muted: Color32,
    pub text_faint: Color32,
    pub accent: Color32,
    pub accent_hover: Color32,
    pub accent_soft: Color32,
    pub accent_faint: Color32,
    pub selection: Color32,
    pub selection_border: Color32,
    pub on_accent: Color32,
    pub row_hover: Color32,
    pub zebra: Color32,
    pub search_hit: Color32,
    pub kind_text: Color32,
    pub kind_number: Color32,
    pub kind_date: Color32,
    pub kind_bool: Color32,
    pub danger: Color32,
    pub success: Color32,
    pub warning: Color32,
    pub shadow: Color32,
}

const fn rgb(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

const fn rgba(hex: u32, a: u8) -> Color32 {
    Color32::from_rgba_premultiplied(
        (((hex >> 16) & 0xff) * a as u32 / 255) as u8,
        (((hex >> 8) & 0xff) * a as u32 / 255) as u8,
        ((hex & 0xff) * a as u32 / 255) as u8,
        a,
    )
}

pub const DARK: Palette = Palette {
    dark: true,
    bg: rgb(0x0E1014),
    panel: rgb(0x13161C),
    surface: rgb(0x181C23),
    surface_hover: rgb(0x20252E),
    elevated: rgb(0x1A1E26),
    input_bg: rgb(0x0F1216),
    border: rgb(0x252A34),
    border_strong: rgb(0x343B48),
    grid_line: rgb(0x1B1F27),
    text: rgb(0xE8EAF0),
    text_muted: rgb(0xA2AABA),
    text_faint: rgb(0x687184),
    accent: rgb(0x8F7FFF),
    accent_hover: rgb(0xA396FF),
    accent_soft: rgba(0x8F7FFF, 52),
    accent_faint: rgba(0x8F7FFF, 22),
    selection: rgba(0x8F7FFF, 34),
    selection_border: rgba(0x8F7FFF, 150),
    on_accent: rgb(0xFFFFFF),
    row_hover: rgba(0xFFFFFF, 11),
    zebra: rgba(0xFFFFFF, 8),
    search_hit: rgba(0xFFC21A, 78),
    kind_text: rgb(0x9AA4B6),
    kind_number: rgb(0x5CC8F5),
    kind_date: rgb(0xFFB46E),
    kind_bool: rgb(0x6EE7A8),
    danger: rgb(0xFF6B6B),
    success: rgb(0x4ADE80),
    warning: rgb(0xFBBF24),
    shadow: rgba(0x000000, 110),
};

pub const LIGHT: Palette = Palette {
    dark: false,
    bg: rgb(0xFFFFFF),
    panel: rgb(0xF6F7F9),
    surface: rgb(0xF1F3F6),
    surface_hover: rgb(0xE8EBF0),
    elevated: rgb(0xFFFFFF),
    input_bg: rgb(0xFFFFFF),
    border: rgb(0xDFE2E8),
    border_strong: rgb(0xC9CED7),
    grid_line: rgb(0xEDEFF3),
    text: rgb(0x171A21),
    text_muted: rgb(0x535C6D),
    text_faint: rgb(0x8A93A4),
    accent: rgb(0x6551F5),
    accent_hover: rgb(0x5641EB),
    accent_soft: rgba(0x6551F5, 40),
    accent_faint: rgba(0x6551F5, 16),
    selection: rgba(0x6551F5, 30),
    selection_border: rgba(0x6551F5, 140),
    on_accent: rgb(0xFFFFFF),
    row_hover: rgba(0x1E2430, 12),
    zebra: rgba(0x1E2430, 9),
    search_hit: rgba(0xFFB800, 96),
    kind_text: rgb(0x6B7487),
    kind_number: rgb(0x0B82C4),
    kind_date: rgb(0xD5701A),
    kind_bool: rgb(0x16955A),
    danger: rgb(0xE5484D),
    success: rgb(0x16A34A),
    warning: rgb(0xD97706),
    shadow: rgba(0x101828, 40),
};

impl Palette {
    pub fn of(ui: &Ui) -> &'static Palette {
        if ui.visuals().dark_mode { &DARK } else { &LIGHT }
    }

    pub fn of_ctx(ctx: &egui::Context) -> &'static Palette {
        match ctx.theme() {
            Theme::Dark => &DARK,
            Theme::Light => &LIGHT,
        }
    }

    pub fn kind_color(&self, kind: ColumnKind) -> Color32 {
        match kind {
            ColumnKind::Text => self.kind_text,
            ColumnKind::Integer | ColumnKind::Decimal => self.kind_number,
            ColumnKind::Date | ColumnKind::DateTime => self.kind_date,
            ColumnKind::Bool => self.kind_bool,
            ColumnKind::Empty => self.text_faint,
        }
    }
}

pub fn kind_icon(kind: ColumnKind) -> &'static str {
    use egui_phosphor::regular as ic;
    match kind {
        ColumnKind::Text => ic::TEXT_AA,
        ColumnKind::Integer => ic::HASH,
        ColumnKind::Decimal => ic::HASH,
        ColumnKind::Date => ic::CALENDAR_BLANK,
        ColumnKind::DateTime => ic::CLOCK_COUNTER_CLOCKWISE,
        ColumnKind::Bool => ic::TOGGLE_LEFT,
        ColumnKind::Empty => ic::CIRCLE_DASHED,
    }
}

/// Carrega Inter (texto), JetBrains Mono (monoespaçada) e os ícones Phosphor.
pub fn install_fonts(ctx: &egui::Context) {
    const INTER: &[u8] = include_bytes!("../assets/fonts/InterVariable.ttf");
    const MONO: &[u8] = include_bytes!("../assets/fonts/JetBrainsMono.ttf");

    let mut fonts = FontDefinitions::default();
    let weight = |w: f32| FontTweak {
        coords: VariationCoords::new([(b"wght", w)]),
        ..Default::default()
    };
    let mut add = |name: &str, bytes: &'static [u8], w: f32| {
        fonts.font_data.insert(
            name.to_owned(),
            Arc::new(FontData::from_static(bytes).tweak(weight(w))),
        );
    };
    add("inter", INTER, 420.0);
    add("inter-medium", INTER, 530.0);
    add("inter-semibold", INTER, 620.0);
    add("jetbrains", MONO, 400.0);

    // Fontes padrão do egui ficam como reserva (emojis, símbolos).
    let fallbacks = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    let mut proportional = vec!["inter".to_owned()];
    proportional.extend(fallbacks.iter().cloned());
    fonts
        .families
        .insert(FontFamily::Proportional, proportional);
    fonts
        .families
        .entry(FontFamily::Monospace)
        .or_default()
        .insert(0, "jetbrains".to_owned());

    // Ícones numa família própria, com a Phosphor em primeiro lugar: a Inter tem
    // glifos na mesma faixa de uso privado do Unicode e "roubaria" os ícones.
    fonts.font_data.insert(
        "phosphor".to_owned(),
        Arc::new(FontData::from_static(
            egui_phosphor::Variant::Regular.font_bytes(),
        )),
    );
    let mut icons = vec!["phosphor".to_owned()];
    icons.extend(fallbacks.iter().cloned());
    fonts.families.insert(FontFamily::Name(ICONS.into()), icons);

    for (family, face) in [(MEDIUM, "inter-medium"), (SEMIBOLD, "inter-semibold")] {
        let mut list = vec![face.to_owned()];
        list.extend(fallbacks.iter().cloned());
        fonts.families.insert(FontFamily::Name(family.into()), list);
    }
    ctx.set_fonts(fonts);
}

fn widget(bg: Color32, stroke: Stroke, fg: Color32, radius: u8) -> WidgetVisuals {
    WidgetVisuals {
        bg_fill: bg,
        weak_bg_fill: bg,
        bg_stroke: stroke,
        corner_radius: CornerRadius::same(radius),
        fg_stroke: Stroke::new(1.0, fg),
        expansion: 0.0,
    }
}

pub fn make_style(p: &Palette) -> egui::Style {
    let mut style = egui::Style::default();
    let mut v = if p.dark {
        Visuals::dark()
    } else {
        Visuals::light()
    };

    v.panel_fill = p.panel;
    v.window_fill = p.elevated;
    v.window_stroke = Stroke::new(1.0, p.border);
    v.window_corner_radius = CornerRadius::same(12);
    v.menu_corner_radius = CornerRadius::same(10);
    v.window_shadow = Shadow {
        offset: [0, 10],
        blur: 32,
        spread: 0,
        color: p.shadow,
    };
    v.popup_shadow = Shadow {
        offset: [0, 6],
        blur: 22,
        spread: 0,
        color: p.shadow,
    };
    v.extreme_bg_color = p.input_bg;
    v.text_edit_bg_color = Some(p.input_bg);
    v.faint_bg_color = p.surface;
    v.code_bg_color = p.surface;
    v.hyperlink_color = p.accent;
    v.warn_fg_color = p.warning;
    v.error_fg_color = p.danger;
    v.selection = Selection {
        bg_fill: p.accent_soft,
        stroke: Stroke::new(1.0, p.accent),
    };
    v.text_cursor.stroke = Stroke::new(2.0, p.accent);
    v.striped = false;
    v.indent_has_left_vline = false;
    v.collapsing_header_frame = false;
    v.slider_trailing_fill = true;
    v.widgets = Widgets {
        noninteractive: widget(p.panel, Stroke::new(1.0, p.border), p.text, 8),
        inactive: widget(p.surface, Stroke::new(1.0, p.border), p.text, 8),
        hovered: widget(p.surface_hover, Stroke::new(1.0, p.border_strong), p.text, 8),
        active: widget(p.accent_soft, Stroke::new(1.0, p.accent), p.text, 8),
        open: widget(p.surface_hover, Stroke::new(1.0, p.border_strong), p.text, 8),
    };
    style.visuals = v;

    let s = &mut style.spacing;
    s.item_spacing = vec2(8.0, 6.0);
    s.button_padding = vec2(10.0, 5.0);
    s.interact_size = vec2(24.0, 26.0);
    s.menu_margin = Margin::same(6);
    s.window_margin = Margin::same(16);
    s.icon_width = 15.0;
    s.icon_width_inner = 9.0;
    s.icon_spacing = 6.0;
    s.combo_height = 300.0;
    s.scroll = ScrollStyle {
        bar_width: 8.0,
        floating_width: 3.0,
        ..ScrollStyle::floating()
    };

    style.text_styles = [
        (TextStyle::Small, regular(11.5)),
        (TextStyle::Body, regular(13.5)),
        (TextStyle::Button, regular(13.5)),
        (TextStyle::Heading, semibold(20.0)),
        (TextStyle::Monospace, mono(12.5)),
    ]
    .into();

    style.interaction.tooltip_delay = 0.35;
    style.interaction.selectable_labels = false;
    style.animation_time = 0.12;
    style
}

pub fn apply(ctx: &egui::Context, dark: bool) {
    ctx.set_style_of(Theme::Dark, make_style(&DARK));
    ctx.set_style_of(Theme::Light, make_style(&LIGHT));
    ctx.set_theme(if dark { Theme::Dark } else { Theme::Light });
}
