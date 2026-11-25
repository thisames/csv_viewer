//! Barra lateral: lista de colunas e detalhes da coluna selecionada.

use egui::{
    Align, Align2, CornerRadius, Id, Layout, Margin, Rect, RichText, ScrollArea, Sense, Stroke,
    TextEdit, Ui, pos2, vec2,
};
use egui_phosphor::regular as ic;

use crate::data::infer::{DateOrder, DecimalSep, parse_datetime, parse_number};
use crate::data::query::{Filter, FilterOp};
use crate::data::stats::{ColumnStats, NumericStats};
use crate::data::table::{ColumnKind, Table};
use crate::data::text::Needle;
use crate::document::Document;
use crate::format;
use crate::theme::{self, Palette};
use crate::ui::grid::CellPos;
use crate::ui::widgets;

pub fn show(ui: &mut Ui, doc: &mut Document) {
    let Some(table) = doc.table.clone() else {
        return;
    };
    let p = Palette::of(ui);
    let ncols = table.columns.len();
    let hidden_count = doc.hidden.iter().filter(|h| **h).count();

    ui.add_space(12.0);
    ui.horizontal(|ui| {
        widgets::section_label(ui, "Colunas");
        widgets::badge(ui, &format::int(ncols), p.text_muted, p.surface_hover);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if hidden_count > 0 {
                let r = ui.add(
                    egui::Label::new(
                        RichText::new(if hidden_count == 1 {
                            "Mostrar 1 oculta".to_owned()
                        } else {
                            format!("Mostrar {hidden_count} ocultas")
                        })
                            .color(p.accent)
                            .font(theme::medium(12.0)),
                    )
                    .sense(Sense::click()),
                );
                if r.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    doc.hidden.iter_mut().for_each(|h| *h = false);
                }
            }
        });
    });
    ui.add_space(4.0);
    search_field(ui, &mut doc.column_query, "Filtrar colunas…", Id::new(("colq", doc.id)));
    ui.add_space(6.0);

    let selected = doc.selected_col();
    let needle = Needle::new(&doc.column_query);
    let list_h = (ui.available_height() * 0.40).clamp(110.0, 460.0);
    let mut select: Option<usize> = None;
    let mut toggle: Option<usize> = None;
    ScrollArea::vertical()
        .id_salt(("col_list", doc.id))
        .max_height(list_h)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            let mut shown = 0;
            for &c in &doc.order {
                let col = &table.columns[c];
                if !needle.is_empty() && !needle.matches(&col.name) {
                    continue;
                }
                shown += 1;
                let hidden = doc.hidden.get(c).copied().unwrap_or(false);
                let (row, eye) = column_row(ui, &table, c, selected == Some(c), hidden);
                if eye {
                    toggle = Some(c);
                } else if row {
                    select = Some(c);
                }
            }
            if shown == 0 {
                ui.add_space(8.0);
                let msg = if needle.is_empty() { "Nenhuma coluna" } else { "Nenhuma coluna com esse nome" };
                ui.label(RichText::new(msg).color(p.text_faint));
            }
        });
    if let Some(c) = toggle {
        let now_hidden = !doc.hidden.get(c).copied().unwrap_or(false);
        if now_hidden && doc.visible_cols().len() <= 1 {
            // Mantém pelo menos uma coluna visível.
        } else {
            doc.set_hidden(c, now_hidden);
        }
    }
    if let Some(c) = select {
        if doc.hidden.get(c).copied().unwrap_or(false) {
            doc.set_hidden(c, false);
        }
        let row = doc.grid.cursor.map_or(0, |cur| cur.row);
        if !doc.view.is_empty() {
            doc.grid.cursor = Some(CellPos { row, col: c });
            doc.grid.anchor = doc.grid.cursor;
            doc.grid.ensure_visible = true;
        }
    }

    ui.add_space(8.0);
    let sep_y = ui.cursor().min.y;
    ui.painter().hline(
        ui.max_rect().x_range(),
        sep_y,
        Stroke::new(1.0, p.border),
    );
    ui.add_space(2.0);

    ScrollArea::vertical()
        .id_salt(("col_details", doc.id))
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.add_space(10.0);
            match doc.selected_col().filter(|&c| c < ncols) {
                Some(c) => details(ui, doc, &table, c),
                None => {
                    ui.add_space(20.0);
                    ui.vertical_centered(|ui| {
                        ui.label(theme::icon_text(ic::CURSOR_CLICK, 26.0).color(p.text_faint));
                        ui.label(
                            RichText::new("Selecione uma célula para ver\nos detalhes da coluna")
                                .color(p.text_faint),
                        );
                    });
                }
            }
            ui.add_space(16.0);
        });
}

/// Campo de busca com ícone de lupa e botão de limpar.
pub fn search_field(ui: &mut Ui, text: &mut String, hint: &str, id: Id) -> egui::Response {
    let p = Palette::of(ui);
    let width = ui.available_width();
    let out = TextEdit::singleline(text)
        .id(id)
        .hint_text(RichText::new(hint).color(p.text_faint))
        .prefix(theme::icon_text(ic::MAGNIFYING_GLASS, 15.0).color(p.text_faint))
        .margin(Margin::symmetric(8, 5))
        .desired_width(width)
        .show(ui);
    let resp = out.response.response;
    if (resp.has_focus() || resp.lost_focus()) && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        text.clear();
    }
    if !text.is_empty() {
        let r = Rect::from_center_size(pos2(resp.rect.max.x - 14.0, resp.rect.center().y), vec2(18.0, 18.0));
        let x = ui.interact(r, id.with("clear"), Sense::click());
        ui.painter().text(
            r.center(),
            Align2::CENTER_CENTER,
            ic::X_CIRCLE,
            theme::icon(14.0),
            if x.hovered() { p.text } else { p.text_faint },
        );
        if x.clicked() {
            text.clear();
        }
    }
    resp
}

/// Uma linha da lista de colunas. Devolve (clicou na linha, clicou no olho).
fn column_row(ui: &mut Ui, table: &Table, c: usize, selected: bool, hidden: bool) -> (bool, bool) {
    let p = Palette::of(ui);
    let col = &table.columns[c];
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 30.0), Sense::click());
    let eye_rect = Rect::from_center_size(pos2(rect.min.x + 15.0, rect.center().y), vec2(24.0, 24.0));
    let eye = ui.interact(eye_rect, resp.id.with("eye"), Sense::click());

    let bg = if selected {
        p.accent_faint
    } else if resp.hovered() {
        p.surface_hover
    } else {
        egui::Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, CornerRadius::same(7), bg);
    if selected {
        let bar = Rect::from_min_size(rect.min + vec2(0.0, 7.0), vec2(3.0, rect.height() - 14.0));
        ui.painter().rect_filled(bar, CornerRadius::same(2), p.accent);
    }

    let eye_color = if eye.hovered() {
        p.text
    } else if hidden {
        p.text_faint
    } else if resp.hovered() {
        p.text_muted
    } else {
        p.text_faint.gamma_multiply(0.7)
    };
    ui.painter().text(
        eye_rect.center(),
        Align2::CENTER_CENTER,
        if hidden { ic::EYE_SLASH } else { ic::EYE },
        theme::icon(15.0),
        eye_color,
    );
    let icon_x = rect.min.x + 34.0;
    ui.painter().text(
        pos2(icon_x, rect.center().y),
        Align2::LEFT_CENTER,
        theme::kind_icon(col.kind),
        theme::icon(14.0),
        if hidden { p.text_faint } else { p.kind_color(col.kind) },
    );

    // Lado direito: mini-histograma ou % preenchido.
    let mut right = rect.max.x - 8.0;
    let rows = table.rows.max(1);
    if let Some(bins) = &col.profile.sparkline {
        let size = vec2(42.0, 14.0);
        let spark = Rect::from_min_size(pos2(right - size.x, rect.center().y - size.y / 2.0), size);
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(spark));
        widgets::sparkline(&mut child, bins, size, p.kind_color(col.kind).gamma_multiply(if hidden { 0.3 } else { 0.65 }));
        right = spark.min.x - 8.0;
    } else if col.profile.filled < table.rows {
        let txt = format::percent(col.profile.filled, rows);
        let r = ui.painter().text(
            pos2(right, rect.center().y),
            Align2::RIGHT_CENTER,
            txt,
            theme::regular(11.0),
            p.text_faint,
        );
        right = r.min.x - 8.0;
    }

    let name_x = icon_x + 22.0;
    widgets::paint_truncated(
        ui.painter(),
        pos2(name_x, rect.center().y),
        &col.name,
        if selected { theme::medium(13.0) } else { theme::regular(13.0) },
        if hidden { p.text_faint } else { p.text },
        right - name_x,
    );

    let eye_clicked = eye
        .on_hover_text(if hidden { "Mostrar coluna" } else { "Ocultar coluna" })
        .clicked();
    let resp = resp.on_hover_text(format!(
        "{} · {} · {} preenchidos",
        col.name,
        col.kind.label(),
        format::percent(col.profile.filled, rows)
    ));
    (resp.clicked(), eye_clicked)
}

fn details(ui: &mut Ui, doc: &mut Document, table: &Table, c: usize) {
    let p = Palette::of(ui);
    let col = &table.columns[c];

    ui.horizontal(|ui| {
        ui.label(
            theme::icon_text(theme::kind_icon(col.kind), 15.0)
                .size(18.0)
                .color(p.kind_color(col.kind)),
        );
        ui.add(
            egui::Label::new(RichText::new(&col.name).font(theme::semibold(15.5)).color(p.text))
                .wrap(),
        );
    });
    ui.label(
        RichText::new(format!("{}  ·  coluna {}", col.kind.label(), format::column_letter(c)))
            .color(p.text_muted)
            .font(theme::regular(12.0)),
    );
    ui.add_space(12.0);

    let stats = doc.stats_for(c).cloned();
    match &stats {
        Some(s) => stats_view(ui, doc, s),
        None => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(RichText::new("Calculando estatísticas…").color(p.text_muted));
            });
        }
    }

    ui.add_space(16.0);
    filter_section(ui, doc, table, c);
}

fn stats_view(ui: &mut Ui, doc: &mut Document, s: &ColumnStats) {
    let p = Palette::of(ui);
    let w = ui.available_width();

    // Preenchimento.
    ui.horizontal(|ui| {
        ui.label(RichText::new("Preenchimento").color(p.text_muted).font(theme::regular(12.0)));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(
                RichText::new(format::percent(s.filled, s.rows))
                    .font(theme::semibold(12.5))
                    .color(p.text),
            );
        });
    });
    let frac = if s.rows == 0 { 0.0 } else { s.filled as f32 / s.rows as f32 };
    let fill_color = if frac >= 0.999 { p.success } else if frac > 0.6 { p.accent } else { p.warning };
    widgets::ratio_bar(ui, w, frac, fill_color);
    ui.add_space(2.0);
    ui.label(
        RichText::new(format!(
            "{} preenchidos · {} vazios",
            format::int(s.filled),
            format::int(s.rows - s.filled)
        ))
        .color(p.text_faint)
        .font(theme::regular(11.5)),
    );
    ui.add_space(10.0);

    // Cartões.
    let gap = 8.0;
    let tile_w = ((w - gap) / 2.0).floor();
    let mut tiles: Vec<(String, String, Option<String>)> = Vec::new();
    let distinct = if s.distinct_capped {
        format!("> {}", format::int(s.distinct))
    } else {
        format::int(s.distinct)
    };
    tiles.push(("Valores únicos".into(), distinct, None));
    tiles.push(("Linhas".into(), format::int(s.rows), None));
    if let Some(n) = &s.numeric {
        let k = s.kind;
        if k.is_temporal() {
            tiles.push(("Mais antiga".into(), format::key(n.min, k), None));
            tiles.push(("Mais recente".into(), format::key(n.max, k), None));
            let days = (n.max - n.min).round() as i64;
            tiles.push(("Intervalo".into(), format::plural(days.max(0) as usize, "dia", "dias"), None));
            tiles.push(("Mediana".into(), format::key(n.median, k), None));
        } else {
            let fit = |v: f64| format::num_fit(v, 13);
            for (label, v) in [
                ("Mínimo", n.min),
                ("Máximo", n.max),
                ("Soma", n.sum),
                ("Média", n.mean),
                ("Mediana", n.median),
                ("Desvio padrão", n.std_dev),
            ] {
                tiles.push((label.into(), fit(v), Some(format::num(v))));
            }
        }
    } else if let Some((min, max, avg)) = s.text_len {
        tiles.push(("Tamanho médio".into(), format!("{} caract.", format::num((avg * 10.0).round() / 10.0)), None));
        tiles.push(("Mais longo".into(), format!("{} caract.", format::int(max)), Some(format!("Mais curto: {min}"))));
    }
    for pair in tiles.chunks(2) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for (label, value, hint) in pair {
                widgets::stat_tile(ui, tile_w, label, value, hint.as_deref());
            }
        });
        ui.add_space(gap - 6.0);
    }

    // Distribuição.
    if let Some(n) = &s.numeric
        && n.count > 1
        && n.max > n.min
    {
        ui.add_space(10.0);
        widgets::section_label(ui, "Distribuição");
        ui.add_space(4.0);
        histogram(ui, n, s.kind, p.kind_color(s.kind));
    }

    // Mais comuns.
    if !s.top.is_empty() && s.top.iter().any(|(_, c)| *c > 1) {
        ui.add_space(12.0);
        widgets::section_label(ui, "Valores mais comuns");
        ui.add_space(4.0);
        let max = s.top.first().map_or(1, |t| t.1).max(1);
        ui.spacing_mut().item_spacing.y = 2.0;
        for (value, count) in s.top.iter().take(8) {
            let (rect, resp) = ui.allocate_exact_size(vec2(w, 27.0), Sense::click());
            let frac = *count as f32 / max as f32;
            if resp.hovered() {
                ui.painter().rect_filled(rect, CornerRadius::same(6), p.surface_hover);
            }
            let mut bar = rect.shrink2(vec2(0.0, 3.0));
            bar.set_width((rect.width() * frac).max(3.0));
            ui.painter().rect_filled(bar, CornerRadius::same(5), p.kind_color(s.kind).gamma_multiply(0.16));
            let right_text = format!("{}  {}", format::int(*count), format::percent(*count, s.filled.max(1)));
            let rr = ui.painter().text(
                pos2(rect.max.x - 8.0, rect.center().y),
                Align2::RIGHT_CENTER,
                right_text,
                theme::regular(11.5),
                p.text_muted,
            );
            let shown = value.replace(['\n', '\r', '\t'], " ");
            widgets::paint_truncated(
                ui.painter(),
                pos2(rect.min.x + 8.0, rect.center().y),
                &shown,
                theme::regular(12.5),
                p.text,
                rr.min.x - rect.min.x - 16.0,
            );
            if resp
                .on_hover_text("Clique para mostrar só este valor")
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
            {
                doc.add_filter(Filter {
                    col: s.col,
                    op: FilterOp::Equals(value.clone()),
                });
            }
        }
    }
}

fn histogram(ui: &mut Ui, n: &NumericStats, kind: ColumnKind, color: egui::Color32) {
    let p = Palette::of(ui);
    let w = ui.available_width();
    let h = 74.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(w, h), Sense::hover());
    let bins = &n.histogram;
    let max = bins.iter().copied().max().unwrap_or(1).max(1) as f32;
    let bw = w / bins.len() as f32;
    let hovered = resp
        .hover_pos()
        .map(|pos| (((pos.x - rect.min.x) / bw) as usize).min(bins.len() - 1));
    for (i, &b) in bins.iter().enumerate() {
        let x = rect.min.x + i as f32 * bw;
        let col_rect = Rect::from_min_max(pos2(x, rect.min.y), pos2(x + bw, rect.max.y));
        if hovered == Some(i) {
            ui.painter().rect_filled(col_rect, CornerRadius::same(3), p.surface_hover);
        }
        if b == 0 {
            continue;
        }
        let bh = (b as f32 / max * (h - 6.0)).max(2.0);
        let bar = Rect::from_min_max(pos2(x + 1.0, rect.max.y - bh), pos2(x + bw - 1.0, rect.max.y));
        let c = if hovered == Some(i) { p.accent } else { color.gamma_multiply(0.8) };
        ui.painter().rect_filled(bar, CornerRadius { nw: 3, ne: 3, sw: 0, se: 0 }, c);
    }
    ui.painter().hline(rect.x_range(), rect.max.y + 0.5, Stroke::new(1.0, p.border));
    if let Some(i) = hovered {
        let span = (n.max - n.min) / bins.len() as f64;
        let lo = n.min + span * i as f64;
        let hi = lo + span;
        resp.on_hover_text(format!(
            "{} – {}\n{}",
            format::key(lo, kind),
            format::key(hi, kind),
            format::plural(bins[i] as usize, "linha", "linhas")
        ));
    }
    ui.horizontal(|ui| {
        ui.label(RichText::new(format::key(n.min, kind)).color(p.text_faint).font(theme::regular(11.0)));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(RichText::new(format::key(n.max, kind)).color(p.text_faint).font(theme::regular(11.0)));
        });
    });
}

fn describe(op: &FilterOp, kind: ColumnKind) -> (&'static str, String) {
    let short = |s: &str| {
        let t: String = s.chars().take(30).collect();
        if t.len() < s.len() { format!("{t}…") } else { t }
    };
    match op {
        FilterOp::Contains(s) => (ic::MAGNIFYING_GLASS, format!("contém “{}”", short(s))),
        FilterOp::Equals(s) => (ic::EQUALS, format!("= “{}”", short(s))),
        FilterOp::NotEquals(s) => (ic::PROHIBIT, format!("≠ “{}”", short(s))),
        FilterOp::Empty => (ic::CIRCLE_DASHED, "vazio".to_owned()),
        FilterOp::NotEmpty => (ic::CHECK_CIRCLE, "não vazio".to_owned()),
        FilterOp::AtLeast(v) => (ic::ARROW_LINE_UP, format!("≥ {}", format::key(*v, kind))),
        FilterOp::AtMost(v) => (ic::ARROW_LINE_DOWN, format!("≤ {}", format::key(*v, kind))),
    }
}

/// Descrição curta de um filtro, para os chips no topo.
pub fn filter_label(table: &Table, f: &Filter) -> (&'static str, String) {
    let col = &table.columns[f.col];
    let (icon, text) = describe(&f.op, col.kind);
    (icon, format!("{} {}", col.name, text))
}

fn parse_bound(s: &str, kind: ColumnKind) -> Option<f64> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    if kind.is_temporal() {
        return parse_datetime(t, DateOrder::Dmy).map(|(v, _)| v);
    }
    parse_number(t, DecimalSep::Comma).or_else(|| parse_number(t, DecimalSep::Dot))
}

fn filter_section(ui: &mut Ui, doc: &mut Document, table: &Table, c: usize) {
    let p = Palette::of(ui);
    let kind = table.columns[c].kind;
    if doc.filter_input_col != Some(c) {
        doc.filter_input_col = Some(c);
        doc.contains_input.clear();
        doc.min_input.clear();
        doc.max_input.clear();
    }
    widgets::section_label(ui, "Filtrar esta coluna");
    ui.add_space(4.0);

    // Filtros ativos desta coluna.
    let mut remove = None;
    for (i, f) in doc.spec.filters.iter().enumerate().filter(|(_, f)| f.col == c) {
        let (icon, text) = describe(&f.op, kind);
        ui.horizontal(|ui| {
            let (_, x) = widgets::filter_chip(ui, icon, &text);
            if x {
                remove = Some(i);
            }
        });
    }
    if let Some(i) = remove {
        doc.remove_filter(i);
    }

    let id = Id::new(("contains", doc.id));
    let w = ui.available_width();
    ui.horizontal(|ui| {
        let resp = TextEdit::singleline(&mut doc.contains_input)
            .id(id)
            .hint_text(RichText::new("Contém o texto…").color(p.text_faint))
            .margin(Margin::symmetric(8, 5))
            .desired_width(w - 40.0)
            .show(ui)
            .response
            .response;
        if doc.focus_column_filter {
            resp.request_focus();
            doc.focus_column_filter = false;
        }
        let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        let add = widgets::icon_button(ui, ic::PLUS, "Adicionar filtro (Enter)", false).clicked();
        if (enter || add) && !doc.contains_input.trim().is_empty() {
            let text = doc.contains_input.trim().to_owned();
            doc.add_filter(Filter {
                col: c,
                op: FilterOp::Contains(text),
            });
            doc.contains_input.clear();
        }
    });

    if kind.is_numeric() || kind.is_temporal() {
        ui.add_space(4.0);
        let hint = if kind.is_temporal() { ("de (dd/mm/aaaa)", "até") } else { ("mínimo", "máximo") };
        let half = ((w - 40.0 - 8.0) / 2.0).floor();
        let mut apply = false;
        ui.horizontal(|ui| {
            let min_ok = doc.min_input.trim().is_empty() || parse_bound(&doc.min_input, kind).is_some();
            let max_ok = doc.max_input.trim().is_empty() || parse_bound(&doc.max_input, kind).is_some();
            for (text, ok, h) in [(&mut doc.min_input, min_ok, hint.0), (&mut doc.max_input, max_ok, hint.1)] {
                let mut te = TextEdit::singleline(text)
                    .hint_text(RichText::new(h).color(p.text_faint))
                    .margin(Margin::symmetric(8, 5))
                    .desired_width(half);
                if !ok {
                    te = te.text_color(p.danger);
                }
                let r = te.show(ui).response.response;
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    apply = true;
                }
            }
            if widgets::icon_button(ui, ic::PLUS, "Aplicar intervalo (Enter)", false).clicked() {
                apply = true;
            }
        });
        if apply {
            if let Some(v) = parse_bound(&doc.min_input, kind) {
                doc.add_filter(Filter { col: c, op: FilterOp::AtLeast(v) });
                doc.min_input.clear();
            }
            if let Some(v) = parse_bound(&doc.max_input, kind) {
                doc.add_filter(Filter { col: c, op: FilterOp::AtMost(v) });
                doc.max_input.clear();
            }
        }
    }

    ui.add_space(6.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
        let mut quick = |ui: &mut Ui, icon: &str, label: &str, op: FilterOp| {
            let r = ui.add(
                egui::Button::new((
                    theme::icon_text(icon, 13.0),
                    RichText::new(label).font(theme::regular(12.0)),
                ))
                .gap(6.0)
                    .corner_radius(CornerRadius::same(12))
                    .min_size(vec2(0.0, 24.0)),
            );
            if r.clicked() {
                doc.add_filter(Filter { col: c, op });
            }
        };
        quick(ui, ic::CHECK_CIRCLE, "Não vazios", FilterOp::NotEmpty);
        quick(ui, ic::CIRCLE_DASHED, "Só vazios", FilterOp::Empty);
    });
}
