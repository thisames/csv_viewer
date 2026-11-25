//! Grade de dados virtualizada (linhas e colunas): só o que está visível é
//! desenhado, então funciona com milhões de linhas e centenas de colunas.
//!
//! Cabeçalho e numeração de linhas ficam fixos; a rolagem é própria (em f64)
//! para não perder precisão com arquivos enormes.

use std::borrow::Cow;
use std::ops::RangeInclusive;

use egui::epaint::text::{LayoutJob, TextFormat, TextWrapping};
use egui::{
    Align, Align2, Color32, CornerRadius, CursorIcon, Event, EventFilter, FontId, Id, Key, Mesh,
    Pos2, Rect, Response, Sense, Shape, Stroke, StrokeKind, Ui, Vec2, pos2, vec2,
};
use egui_phosphor::regular as ic;

use crate::data::export::selection_tsv;
use crate::data::query::{Filter, FilterOp, Sort, View};
use crate::data::table::{Column, ColumnKind, Table};
use crate::data::text::{Needle, truncate_chars};
use crate::format;
use crate::theme::{self, Palette};

const HEADER_H: f32 = 38.0;
const PAD_X: f32 = 10.0;
pub const MIN_COL_W: f32 = 48.0;
const MAX_COL_W: f32 = 1600.0;
const BAR_HIT: f32 = 12.0;
const END_PAD: f32 = 14.0;
/// Máximo de células copiadas de uma vez.
const MAX_COPY_CELLS: usize = 5_000_000;

pub fn row_height(compact: bool) -> f32 {
    if compact { 25.0 } else { 31.0 }
}

/// Posição de uma célula: linha na visão + coluna da tabela.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CellPos {
    pub row: usize,
    pub col: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum Drag {
    #[default]
    None,
    Selecting,
    RowSelecting,
    Resizing {
        col: usize,
        start_w: f32,
        start_x: f32,
    },
    VBar {
        grab: f32,
    },
    HBar {
        grab: f32,
    },
}

#[derive(Default)]
pub struct GridState {
    pub scroll_x: f32,
    pub scroll_y: f64,
    /// Largura por coluna da tabela (0 = ainda não calculada).
    pub widths: Vec<f32>,
    pub cursor: Option<CellPos>,
    pub anchor: Option<CellPos>,
    pub ensure_visible: bool,
    /// Coluna para ajustar a largura ao conteúdo no próximo quadro.
    pub pending_autofit: Option<usize>,
    drag: Drag,
}

/// O que a grade precisa saber para se desenhar.
pub struct GridCtx<'a> {
    pub id: Id,
    pub table: &'a Table,
    pub view: &'a View,
    /// Colunas visíveis, na ordem de exibição (índices da tabela).
    pub cols: &'a [usize],
    pub sort: Option<Sort>,
    pub needle: Option<&'a Needle>,
    pub filtered_cols: &'a [usize],
    pub compact: bool,
    pub zebra: bool,
}

/// Pedidos da grade para o documento/aplicativo.
pub enum GridAction {
    Sort(Option<Sort>),
    AddFilter(Filter),
    ClearFilters,
    HideColumn(usize),
    FocusColumnFilter(usize),
    OpenCell { row: usize, col: usize },
    Toast(String),
}

impl GridState {
    pub fn width(&self, col: usize) -> f32 {
        self.widths.get(col).copied().unwrap_or(120.0)
    }

    /// Seleção retangular: (linhas da visão, posições de exibição).
    pub fn selection(&self, cols: &[usize]) -> Option<(RangeInclusive<usize>, RangeInclusive<usize>)> {
        let c = self.cursor?;
        let a = self.anchor.unwrap_or(c);
        let ci = cols.iter().position(|&x| x == c.col)?;
        let ai = cols.iter().position(|&x| x == a.col).unwrap_or(ci);
        Some((c.row.min(a.row)..=c.row.max(a.row), ci.min(ai)..=ci.max(ai)))
    }

    pub fn select_all(&mut self, rows: usize, cols: &[usize]) {
        if rows == 0 || cols.is_empty() {
            return;
        }
        self.anchor = Some(CellPos {
            row: 0,
            col: cols[0],
        });
        self.cursor = Some(CellPos {
            row: rows - 1,
            col: cols[cols.len() - 1],
        });
    }

    pub fn clear_selection(&mut self) {
        self.cursor = None;
        self.anchor = None;
    }

    /// Mantém o cursor na mesma linha de dados quando a visão muda.
    pub fn remap(&mut self, old: &View, new: &View) {
        let Some(c) = self.cursor else { return };
        let mapped = (c.row < old.len())
            .then(|| old.row(c.row))
            .and_then(|data_row| new.position_of(data_row));
        match mapped {
            Some(row) => {
                self.cursor = Some(CellPos { row, col: c.col });
                self.anchor = self.cursor;
            }
            None if new.is_empty() => self.clear_selection(),
            None => {
                let row = c.row.min(new.len() - 1);
                self.cursor = Some(CellPos { row, col: c.col });
                self.anchor = self.cursor;
            }
        }
    }

    /// Calcula larguras iniciais para colunas novas.
    pub fn ensure_widths(&mut self, ui: &Ui, table: &Table) {
        if self.widths.len() == table.columns.len() && self.widths.iter().all(|&w| w > 0.0) {
            return;
        }
        self.widths.resize(table.columns.len(), 0.0);
        let char_w = ui.fonts_mut(|f| {
            f.layout_no_wrap(
                "0123456789abcdefghijklmnopqrstuvwxyz".to_owned(),
                cell_font(ColumnKind::Text),
                Color32::WHITE,
            )
            .size()
            .x
        }) / 36.0;
        for (i, col) in table.columns.iter().enumerate() {
            if self.widths[i] > 0.0 {
                continue;
            }
            let header_w = ui.fonts_mut(|f| {
                f.layout_no_wrap(col.name.clone(), header_font(), Color32::WHITE)
                    .size()
                    .x
            }) + 2.0 * PAD_X
                + 42.0;
            let content_w = col.typical_chars as f32 * char_w + 2.0 * PAD_X + 6.0;
            self.widths[i] = header_w.max(content_w).clamp(72.0, 380.0).round();
        }
    }
}

fn cell_font(kind: ColumnKind) -> FontId {
    if kind.is_numeric() {
        theme::mono(12.5)
    } else {
        theme::regular(13.0)
    }
}

fn header_font() -> FontId {
    theme::semibold(12.5)
}

/// Texto da célula em uma linha só (quebras viram ↵).
fn display_text(text: &str) -> Cow<'_, str> {
    let t = truncate_chars(text, 400);
    if t.contains(['\n', '\r', '\t']) {
        Cow::Owned(
            t.replace("\r\n", "\n")
                .replace(['\n', '\r'], " ↵ ")
                .replace('\t', " "),
        )
    } else {
        Cow::Borrowed(t)
    }
}

fn cell_job(
    text: &str,
    font: FontId,
    color: Color32,
    max_w: f32,
    needle: Option<&Needle>,
    hit_bg: Color32,
) -> LayoutJob {
    let mut job = LayoutJob {
        wrap: TextWrapping::truncate_at_width(max_w),
        ..Default::default()
    };
    let fmt = TextFormat {
        font_id: font,
        color,
        valign: Align::Center,
        ..Default::default()
    };
    let hits = needle.map(|n| n.find_all(text)).unwrap_or_default();
    if hits.is_empty() {
        job.append(text, 0.0, fmt);
        return job;
    }
    let hit_fmt = TextFormat {
        background: hit_bg,
        expand_bg: 1.5,
        ..fmt.clone()
    };
    let mut last = 0;
    for (s, e) in hits {
        if s > last {
            job.append(&text[last..s], 0.0, fmt.clone());
        }
        job.append(&text[s..e], 0.0, hit_fmt.clone());
        last = e;
    }
    if last < text.len() {
        job.append(&text[last..], 0.0, fmt);
    }
    job
}

/// Gradiente vertical (sombra sob o cabeçalho) ou horizontal (ao lado da numeração).
fn gradient(painter: &egui::Painter, rect: Rect, from: Color32, vertical: bool) {
    let mut mesh = Mesh::default();
    let clear = Color32::TRANSPARENT;
    if vertical {
        mesh.colored_vertex(rect.left_top(), from);
        mesh.colored_vertex(rect.right_top(), from);
        mesh.colored_vertex(rect.left_bottom(), clear);
        mesh.colored_vertex(rect.right_bottom(), clear);
    } else {
        mesh.colored_vertex(rect.left_top(), from);
        mesh.colored_vertex(rect.right_top(), clear);
        mesh.colored_vertex(rect.left_bottom(), from);
        mesh.colored_vertex(rect.right_bottom(), clear);
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(1, 2, 3);
    painter.add(Shape::mesh(mesh));
}

struct Layout {
    rect: Rect,
    header: Rect,
    gutter: Rect,
    body: Rect,
    corner: Rect,
    row_h: f32,
    xs: Vec<f32>,
}

impl Layout {
    fn content_w(&self) -> f32 {
        *self.xs.last().unwrap_or(&0.0)
    }
    fn max_scroll_x(&self) -> f32 {
        (self.content_w() + END_PAD - self.body.width()).max(0.0)
    }
    fn max_scroll_y(&self, rows: usize) -> f64 {
        (rows as f64 * self.row_h as f64 + END_PAD as f64 - self.body.height() as f64).max(0.0)
    }
}

fn compute_xs(st: &GridState, cols: &[usize]) -> Vec<f32> {
    let mut xs = Vec::with_capacity(cols.len() + 1);
    let mut x = 0.0;
    xs.push(x);
    for &c in cols {
        x += st.width(c);
        xs.push(x);
    }
    xs
}

pub fn show(ui: &mut Ui, st: &mut GridState, g: &GridCtx<'_>) -> Vec<GridAction> {
    let p = Palette::of(ui);
    let mut actions = Vec::new();
    let rect = ui.available_rect_before_wrap();
    ui.allocate_rect(rect, Sense::hover());
    st.ensure_widths(ui, g.table);

    let n_rows = g.view.len();
    let ncols = g.cols.len();
    let row_h = row_height(g.compact);
    let digits = g.table.rows.max(1).to_string().len() as f32;
    let gutter_w = (digits * 7.6 + 28.0).max(52.0).round();

    let mut lay = Layout {
        rect,
        header: Rect::from_min_max(pos2(rect.min.x + gutter_w, rect.min.y), pos2(rect.max.x, rect.min.y + HEADER_H)),
        gutter: Rect::from_min_max(pos2(rect.min.x, rect.min.y + HEADER_H), pos2(rect.min.x + gutter_w, rect.max.y)),
        body: Rect::from_min_max(pos2(rect.min.x + gutter_w, rect.min.y + HEADER_H), rect.max),
        corner: Rect::from_min_size(rect.min, vec2(gutter_w, HEADER_H)),
        row_h,
        xs: compute_xs(st, g.cols),
    };
    if let Some(tc) = st.pending_autofit.take()
        && tc < g.table.columns.len()
    {
        autofit(ui, st, g, tc, &lay);
        lay.xs = compute_xs(st, g.cols);
    }

    // ---------------------------------------------------------------- rolagem
    let body_id = g.id.with("body");
    if ui.rect_contains_pointer(rect) && !matches!(st.drag, Drag::VBar { .. } | Drag::HBar { .. }) {
        let delta = ui.input(|i| i.smooth_scroll_delta);
        if delta != Vec2::ZERO {
            st.scroll_x -= delta.x;
            st.scroll_y -= delta.y as f64;
            ui.input_mut(|i| i.smooth_scroll_delta = Vec2::ZERO);
        }
    }

    // ---------------------------------------------------------------- corpo
    let body_resp = ui.interact(lay.body, body_id, Sense::click_and_drag());
    let cell_at = |st: &GridState, lay: &Layout, pos: Pos2, clamp: bool| -> Option<CellPos> {
        if n_rows == 0 || ncols == 0 {
            return None;
        }
        let pos = if clamp {
            pos.clamp(lay.body.min, lay.body.max - vec2(1.0, 1.0))
        } else {
            pos
        };
        let cx = pos.x - lay.body.min.x + st.scroll_x;
        let cy = (pos.y - lay.body.min.y) as f64 + st.scroll_y;
        if cx < 0.0 || cy < 0.0 {
            return None;
        }
        let row = (cy / row_h as f64) as usize;
        let ci = lay.xs.partition_point(|&x| x <= cx).saturating_sub(1);
        if !clamp && (row >= n_rows || ci >= ncols) {
            return None;
        }
        Some(CellPos {
            row: row.min(n_rows - 1),
            col: g.cols[ci.min(ncols - 1)],
        })
    };

    let (primary_pressed, primary_down, shift, latest) = ui.input(|i| {
        (
            i.pointer.primary_pressed(),
            i.pointer.primary_down(),
            i.modifiers.shift,
            i.pointer.latest_pos(),
        )
    });

    if body_resp.is_pointer_button_down_on() && primary_pressed {
        if let Some(pos) = latest
            && let Some(cell) = cell_at(st, &lay, pos, false)
        {
            if shift && st.cursor.is_some() {
                if st.anchor.is_none() {
                    st.anchor = st.cursor;
                }
                st.cursor = Some(cell);
            } else {
                st.cursor = Some(cell);
                st.anchor = Some(cell);
            }
            st.drag = Drag::Selecting;
        }
        ui.memory_mut(|m| m.request_focus(body_id));
    }
    if body_resp.secondary_clicked() {
        ui.memory_mut(|m| m.request_focus(body_id));
        if let Some(pos) = body_resp.interact_pointer_pos()
            && let Some(cell) = cell_at(st, &lay, pos, false)
        {
            let inside = st.selection(g.cols).is_some_and(|(rr, cr)| {
                let ci = g.cols.iter().position(|&x| x == cell.col).unwrap_or(usize::MAX);
                rr.contains(&cell.row) && cr.contains(&ci)
            });
            if !inside {
                st.cursor = Some(cell);
                st.anchor = Some(cell);
            }
        }
    }
    if matches!(st.drag, Drag::Selecting | Drag::RowSelecting) {
        if primary_down {
            if let Some(pos) = latest {
                if let Some(cell) = cell_at(st, &lay, pos, true) {
                    st.cursor = Some(if st.drag == Drag::RowSelecting {
                        CellPos {
                            row: cell.row,
                            col: st.cursor.map_or(cell.col, |c| c.col),
                        }
                    } else {
                        cell
                    });
                }
                // Rolagem automática ao arrastar para fora.
                let dt = ui.input(|i| i.stable_dt).min(0.05);
                let speed = |d: f32| (d * 12.0).clamp(-2400.0, 2400.0) * dt;
                if pos.y < lay.body.top() {
                    st.scroll_y += speed(pos.y - lay.body.top()) as f64;
                } else if pos.y > lay.body.bottom() {
                    st.scroll_y += speed(pos.y - lay.body.bottom()) as f64;
                }
                if st.drag == Drag::Selecting {
                    if pos.x < lay.body.left() {
                        st.scroll_x += speed(pos.x - lay.body.left());
                    } else if pos.x > lay.body.right() {
                        st.scroll_x += speed(pos.x - lay.body.right());
                    }
                }
                if !lay.body.contains(pos) {
                    ui.ctx().request_repaint();
                }
            }
        } else {
            st.drag = Drag::None;
        }
    }
    if body_resp.double_clicked()
        && let Some(c) = st.cursor
    {
        actions.push(GridAction::OpenCell {
            row: g.view.row(c.row),
            col: c.col,
        });
    }

    // ---------------------------------------------------------------- teclado
    let has_focus = ui.memory(|m| m.has_focus(body_id));
    if has_focus {
        ui.memory_mut(|m| {
            m.set_focus_lock_filter(
                body_id,
                EventFilter {
                    tab: false,
                    horizontal_arrows: true,
                    vertical_arrows: true,
                    escape: false,
                },
            )
        });
        handle_keys(ui, st, g, &lay, &mut actions);
    }

    // ---------------------------------------------------------------- numeração
    let gutter_resp = ui.interact(lay.gutter, g.id.with("gutter"), Sense::click_and_drag());
    if gutter_resp.is_pointer_button_down_on() && primary_pressed && ncols > 0 {
        if let Some(pos) = latest {
            let cy = (pos.y - lay.body.min.y) as f64 + st.scroll_y;
            let row = (cy / row_h as f64) as usize;
            if row < n_rows {
                let first = g.cols[0];
                let last = g.cols[ncols - 1];
                if shift && st.anchor.is_some() {
                    st.anchor = st.anchor.map(|a| CellPos { row: a.row, col: first });
                } else {
                    st.anchor = Some(CellPos { row, col: first });
                }
                st.cursor = Some(CellPos { row, col: last });
                st.drag = Drag::RowSelecting;
            }
        }
        ui.memory_mut(|m| m.request_focus(body_id));
    }

    // Canto: selecionar tudo.
    let corner_resp = ui
        .interact(lay.corner, g.id.with("corner"), Sense::click())
        .on_hover_text("Selecionar tudo (Ctrl+A)");
    if corner_resp.clicked() {
        st.select_all(n_rows, g.cols);
        ui.memory_mut(|m| m.request_focus(body_id));
    }

    // ---------------------------------------------------------------- cabeçalho
    header_interactions(ui, st, g, &lay, &mut actions);
    lay.xs = compute_xs(st, g.cols);

    // ---------------------------------------------------------------- barras de rolagem
    let v_track = Rect::from_min_max(
        pos2(lay.body.right() - BAR_HIT, lay.body.top()),
        pos2(lay.body.right(), lay.body.bottom() - BAR_HIT),
    );
    let h_track = Rect::from_min_max(
        pos2(lay.body.left(), lay.body.bottom() - BAR_HIT),
        pos2(lay.body.right() - BAR_HIT, lay.body.bottom()),
    );
    let content_h = n_rows as f64 * row_h as f64 + END_PAD as f64;
    let content_w = lay.content_w() + END_PAD;
    let show_v = content_h > lay.body.height() as f64;
    let show_h = content_w > lay.body.width();
    let v_resp = show_v.then(|| ui.interact(v_track, g.id.with("vbar"), Sense::click_and_drag()));
    let h_resp = show_h.then(|| ui.interact(h_track, g.id.with("hbar"), Sense::click_and_drag()));

    let v_handle = |st: &GridState, lay: &Layout| -> (f32, f32) {
        let track = v_track.height();
        let frac = (lay.body.height() as f64 / content_h).min(1.0) as f32;
        let len = (track * frac).max(32.0).min(track);
        let max = lay.max_scroll_y(n_rows);
        let t = if max > 0.0 { (st.scroll_y / max) as f32 } else { 0.0 };
        (v_track.top() + t * (track - len), len)
    };
    let h_handle = |st: &GridState, lay: &Layout| -> (f32, f32) {
        let track = h_track.width();
        let frac = (lay.body.width() / content_w).min(1.0);
        let len = (track * frac).max(32.0).min(track);
        let max = lay.max_scroll_x();
        let t = if max > 0.0 { st.scroll_x / max } else { 0.0 };
        (h_track.left() + t * (track - len), len)
    };

    if let Some(r) = &v_resp {
        if r.is_pointer_button_down_on() && primary_pressed
            && let Some(pos) = latest
        {
            let (start, len) = v_handle(st, &lay);
            let grab = if pos.y >= start && pos.y <= start + len {
                pos.y - start
            } else {
                len / 2.0
            };
            st.drag = Drag::VBar { grab };
        }
        if let Drag::VBar { grab } = st.drag {
            if primary_down {
                if let Some(pos) = latest {
                    let (_, len) = v_handle(st, &lay);
                    let free = (v_track.height() - len).max(1.0);
                    let t = ((pos.y - grab - v_track.top()) / free).clamp(0.0, 1.0);
                    st.scroll_y = t as f64 * lay.max_scroll_y(n_rows);
                }
            } else {
                st.drag = Drag::None;
            }
        }
    }
    if let Some(r) = &h_resp {
        if r.is_pointer_button_down_on() && primary_pressed
            && let Some(pos) = latest
        {
            let (start, len) = h_handle(st, &lay);
            let grab = if pos.x >= start && pos.x <= start + len {
                pos.x - start
            } else {
                len / 2.0
            };
            st.drag = Drag::HBar { grab };
        }
        if let Drag::HBar { grab } = st.drag {
            if primary_down {
                if let Some(pos) = latest {
                    let (_, len) = h_handle(st, &lay);
                    let free = (h_track.width() - len).max(1.0);
                    let t = ((pos.x - grab - h_track.left()) / free).clamp(0.0, 1.0);
                    st.scroll_x = t * lay.max_scroll_x();
                }
            } else {
                st.drag = Drag::None;
            }
        }
    }

    // ---------------------------------------------------------------- garantir visível
    if st.ensure_visible {
        st.ensure_visible = false;
        if let Some(c) = st.cursor
            && let Some(ci) = g.cols.iter().position(|&x| x == c.col)
        {
            let top = c.row as f64 * row_h as f64;
            let bottom = top + row_h as f64;
            let view_h = (lay.body.height() - if show_h { BAR_HIT } else { 0.0 }) as f64;
            if top < st.scroll_y {
                st.scroll_y = top;
            } else if bottom > st.scroll_y + view_h {
                st.scroll_y = bottom - view_h;
            }
            let (left, right) = (lay.xs[ci], lay.xs[ci + 1]);
            let view_w = lay.body.width() - if show_v { BAR_HIT } else { 0.0 };
            if left < st.scroll_x || right - left > view_w {
                st.scroll_x = left;
            } else if right > st.scroll_x + view_w {
                st.scroll_x = right - view_w;
            }
        }
    }

    st.scroll_x = st.scroll_x.clamp(0.0, lay.max_scroll_x());
    st.scroll_y = st.scroll_y.clamp(0.0, lay.max_scroll_y(n_rows));

    // ---------------------------------------------------------------- menu de contexto
    let ctx_info = st.cursor.map(|c| {
        let column = &g.table.columns[c.col];
        let data_row = g.view.row(c.row.min(n_rows.saturating_sub(1)));
        (c, column, data_row, column.get(data_row).to_owned())
    });
    body_resp.context_menu(|ui| {
        ui.set_min_width(230.0);
        if let Some((c, column, data_row, value)) = &ctx_info {
            cell_menu(ui, st, g, *c, column, *data_row, value, &mut actions);
        }
    });

    // ---------------------------------------------------------------- pintura
    paint(ui, st, g, &lay, p, show_v, show_h, &v_resp, &h_resp, v_handle(st, &lay), h_handle(st, &lay));

    if n_rows == 0 {
        empty_state(ui, &lay, p, g, &mut actions);
    }

    actions
}

fn handle_keys(ui: &mut Ui, st: &mut GridState, g: &GridCtx<'_>, lay: &Layout, actions: &mut Vec<GridAction>) {
    let n_rows = g.view.len();
    let ncols = g.cols.len();
    if n_rows == 0 || ncols == 0 {
        return;
    }
    let page = ((lay.body.height() / lay.row_h).floor() as isize - 1).max(1);
    let events = ui.input(|i| i.events.clone());
    for ev in events {
        match ev {
            Event::Copy => copy_selection(ui, st, g, false, actions),
            Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } => {
                let ext = modifiers.shift;
                let jump = modifiers.command;
                let far = n_rows as isize;
                let far_c = ncols as isize;
                match key {
                    Key::ArrowDown => move_cursor(st, g, if jump { far } else { 1 }, 0, ext),
                    Key::ArrowUp => move_cursor(st, g, if jump { -far } else { -1 }, 0, ext),
                    Key::ArrowRight => move_cursor(st, g, 0, if jump { far_c } else { 1 }, ext),
                    Key::ArrowLeft => move_cursor(st, g, 0, if jump { -far_c } else { -1 }, ext),
                    Key::Tab => move_cursor(st, g, 0, if modifiers.shift { -1 } else { 1 }, false),
                    Key::PageDown => move_cursor(st, g, page, 0, ext),
                    Key::PageUp => move_cursor(st, g, -page, 0, ext),
                    Key::Home if jump => move_cursor(st, g, -far, -far_c, ext),
                    Key::End if jump => move_cursor(st, g, far, far_c, ext),
                    Key::Home => move_cursor(st, g, 0, -far_c, ext),
                    Key::End => move_cursor(st, g, 0, far_c, ext),
                    Key::A if modifiers.command => st.select_all(n_rows, g.cols),
                    Key::Enter | Key::F2 | Key::Space => {
                        if let Some(c) = st.cursor {
                            actions.push(GridAction::OpenCell {
                                row: g.view.row(c.row),
                                col: c.col,
                            });
                        }
                    }
                    Key::Escape => st.anchor = st.cursor,
                    _ => {}
                }
            }
            _ => {}
        }
    }
}

fn move_cursor(st: &mut GridState, g: &GridCtx<'_>, dr: isize, dc: isize, extend: bool) {
    let n_rows = g.view.len() as isize;
    let ncols = g.cols.len() as isize;
    let cur = st.cursor.unwrap_or(CellPos {
        row: 0,
        col: g.cols[0],
    });
    let ci = g.cols.iter().position(|&x| x == cur.col).unwrap_or(0) as isize;
    let row = if st.cursor.is_none() {
        0
    } else {
        (cur.row as isize + dr).clamp(0, n_rows - 1)
    };
    let col = if st.cursor.is_none() {
        0
    } else {
        (ci + dc).clamp(0, ncols - 1)
    };
    let next = CellPos {
        row: row as usize,
        col: g.cols[col as usize],
    };
    if extend {
        if st.anchor.is_none() {
            st.anchor = st.cursor.or(Some(next));
        }
    } else {
        st.anchor = Some(next);
    }
    st.cursor = Some(next);
    st.ensure_visible = true;
}

/// Copia a seleção como TSV (cola direto no Excel / Google Sheets).
pub fn copy_selection(ui: &Ui, st: &GridState, g: &GridCtx<'_>, with_header: bool, actions: &mut Vec<GridAction>) {
    let Some((rows, cr)) = st.selection(g.cols) else {
        return;
    };
    let cols: Vec<usize> = g.cols[cr].to_vec();
    let cells = (rows.end() - rows.start() + 1) * cols.len();
    if cells > MAX_COPY_CELLS {
        actions.push(GridAction::Toast(format!(
            "Seleção grande demais para copiar ({} células). Use Exportar.",
            format::int(cells)
        )));
        return;
    }
    let text = selection_tsv(g.table, g.view, rows, &cols, with_header);
    ui.ctx().copy_text(text);
    actions.push(GridAction::Toast(if cells == 1 {
        "Célula copiada".to_owned()
    } else {
        format!("{} células copiadas", format::int(cells))
    }));
}

#[allow(clippy::too_many_arguments)]
fn cell_menu(
    ui: &mut Ui,
    st: &mut GridState,
    g: &GridCtx<'_>,
    c: CellPos,
    column: &Column,
    data_row: usize,
    value: &str,
    actions: &mut Vec<GridAction>,
) {
    let multi = st
        .selection(g.cols)
        .is_some_and(|(r, cc)| r.start() != r.end() || cc.start() != cc.end());
    if menu_item(ui, ic::COPY, if multi { "Copiar seleção" } else { "Copiar" }, Some("Ctrl+C")) {
        copy_selection(ui, st, g, false, actions);
        ui.close();
    }
    if menu_item(ui, ic::TABLE, "Copiar com cabeçalho", None) {
        copy_selection(ui, st, g, true, actions);
        ui.close();
    }
    if menu_item(ui, ic::ROWS, "Copiar linha inteira", None) {
        let text = selection_tsv(g.table, g.view, c.row..=c.row, g.cols, false);
        ui.ctx().copy_text(text);
        actions.push(GridAction::Toast("Linha copiada".to_owned()));
        ui.close();
    }
    ui.separator();
    let short: String = {
        let t = truncate_chars(value.lines().next().unwrap_or(""), 28);
        if t.len() < value.len() { format!("{t}…") } else { t.to_owned() }
    };
    if !value.is_empty() {
        if menu_item(ui, ic::FUNNEL, &format!("Só “{short}”"), None) {
            actions.push(GridAction::AddFilter(Filter {
                col: c.col,
                op: FilterOp::Equals(value.to_owned()),
            }));
            ui.close();
        }
        if menu_item(ui, ic::PROHIBIT, &format!("Excluir “{short}”"), None) {
            actions.push(GridAction::AddFilter(Filter {
                col: c.col,
                op: FilterOp::NotEquals(value.to_owned()),
            }));
            ui.close();
        }
        if (column.kind.is_numeric() || column.kind.is_temporal())
            && let Some(k) = column.key(data_row)
        {
            let shown = format::key(k, column.kind);
            if menu_item(ui, ic::ARROW_LINE_UP, &format!("Maior ou igual a {shown}"), None) {
                actions.push(GridAction::AddFilter(Filter {
                    col: c.col,
                    op: FilterOp::AtLeast(k),
                }));
                ui.close();
            }
            if menu_item(ui, ic::ARROW_LINE_DOWN, &format!("Menor ou igual a {shown}"), None) {
                actions.push(GridAction::AddFilter(Filter {
                    col: c.col,
                    op: FilterOp::AtMost(k),
                }));
                ui.close();
            }
        }
    }
    if menu_item(ui, ic::BROOM, "Ocultar linhas vazias aqui", None) {
        actions.push(GridAction::AddFilter(Filter {
            col: c.col,
            op: FilterOp::NotEmpty,
        }));
        ui.close();
    }
    ui.separator();
    if menu_item(ui, ic::ARROWS_OUT_SIMPLE, "Ver conteúdo completo", Some("Enter")) {
        actions.push(GridAction::OpenCell {
            row: data_row,
            col: c.col,
        });
        ui.close();
    }
}

/// Item de menu com ícone e atalho opcional.
pub fn menu_item(ui: &mut Ui, icon: &str, label: &str, shortcut: Option<&str>) -> bool {
    let p = Palette::of(ui);
    let mut btn = egui::Button::new((
        theme::icon_text(icon, 15.0).color(p.text_muted),
        label.to_owned(),
    ))
    .gap(10.0)
    .frame_when_inactive(false);
    if let Some(s) = shortcut {
        btn = btn.shortcut_text(s);
    }
    ui.add(btn).clicked()
}

fn header_interactions(
    ui: &mut Ui,
    st: &mut GridState,
    g: &GridCtx<'_>,
    lay: &Layout,
    actions: &mut Vec<GridAction>,
) {
    let ncols = g.cols.len();
    let first = lay.xs.partition_point(|&x| x <= st.scroll_x).saturating_sub(1);
    let last = lay.xs.partition_point(|&x| x < st.scroll_x + lay.header.width()).min(ncols);
    let mut resize_hovered = false;

    for ci in first..last {
        let tc = g.cols[ci];
        let x0 = lay.header.min.x + lay.xs[ci] - st.scroll_x;
        let x1 = lay.header.min.x + lay.xs[ci + 1] - st.scroll_x;
        let cell = Rect::from_min_max(pos2(x0, lay.header.min.y), pos2(x1, lay.header.max.y));
        let visible = cell.intersect(lay.header);
        if visible.width() <= 0.0 {
            continue;
        }
        let resp = ui.interact(visible, g.id.with(("head", tc)), Sense::click());
        if resp.clicked() {
            let next = match g.sort {
                Some(s) if s.col == tc && !s.descending => Some(Sort {
                    col: tc,
                    descending: true,
                }),
                Some(s) if s.col == tc => None,
                _ => Some(Sort {
                    col: tc,
                    descending: false,
                }),
            };
            actions.push(GridAction::Sort(next));
        }
        let resp = resp.on_hover_ui(|ui| {
            let column = &g.table.columns[tc];
            ui.label(egui::RichText::new(&column.name).font(theme::semibold(13.0)));
            ui.label(
                egui::RichText::new(format!(
                    "{}  ·  coluna {}",
                    column.kind.label(),
                    format::column_letter(tc)
                ))
                .color(Palette::of(ui).text_muted)
                .small(),
            );
            ui.label(
                egui::RichText::new("Clique para ordenar · botão direito para opções")
                    .color(Palette::of(ui).text_faint)
                    .small(),
            );
        });
        resp.context_menu(|ui| {
            ui.set_min_width(220.0);
            header_menu(ui, st, g, tc, actions);
        });
    }

    // Alças de redimensionar (por cima das células do cabeçalho).
    for ci in first..last {
        let tc = g.cols[ci];
        let x = lay.header.min.x + lay.xs[ci + 1] - st.scroll_x;
        if x < lay.header.min.x || x > lay.header.max.x + 4.0 {
            continue;
        }
        let handle = Rect::from_min_max(pos2(x - 5.0, lay.header.min.y), pos2(x + 4.0, lay.header.max.y));
        let resp = ui.interact(handle, g.id.with(("resize", tc)), Sense::click_and_drag());
        if resp.hovered() || resp.dragged() {
            resize_hovered = true;
        }
        if resp.drag_started()
            && let Some(pos) = resp.interact_pointer_pos()
        {
            st.drag = Drag::Resizing {
                col: tc,
                start_w: st.width(tc),
                start_x: pos.x,
            };
        }
        if resp.double_clicked() {
            autofit(ui, st, g, tc, lay);
        }
    }
    if let Drag::Resizing { col, start_w, start_x } = st.drag {
        resize_hovered = true;
        let (down, pos) = ui.input(|i| (i.pointer.primary_down(), i.pointer.latest_pos()));
        if down {
            if let Some(pos) = pos
                && let Some(w) = st.widths.get_mut(col)
            {
                *w = (start_w + pos.x - start_x).clamp(MIN_COL_W, MAX_COL_W).round();
            }
        } else {
            st.drag = Drag::None;
        }
    }
    if resize_hovered {
        ui.ctx().set_cursor_icon(CursorIcon::ResizeColumn);
    }
}

fn header_menu(ui: &mut Ui, st: &mut GridState, g: &GridCtx<'_>, tc: usize, actions: &mut Vec<GridAction>) {
    let sorted = g.sort.filter(|s| s.col == tc);
    if menu_item(ui, ic::SORT_ASCENDING, "Ordenar crescente", None) {
        actions.push(GridAction::Sort(Some(Sort { col: tc, descending: false })));
        ui.close();
    }
    if menu_item(ui, ic::SORT_DESCENDING, "Ordenar decrescente", None) {
        actions.push(GridAction::Sort(Some(Sort { col: tc, descending: true })));
        ui.close();
    }
    if sorted.is_some() && menu_item(ui, ic::X, "Remover ordenação", None) {
        actions.push(GridAction::Sort(None));
        ui.close();
    }
    ui.separator();
    if menu_item(ui, ic::FUNNEL, "Filtrar esta coluna…", None) {
        actions.push(GridAction::FocusColumnFilter(tc));
        ui.close();
    }
    if menu_item(ui, ic::ARROWS_OUT_SIMPLE, "Ajustar largura ao conteúdo", None) {
        st.pending_autofit = Some(tc);
        ui.close();
    }
    if menu_item(ui, ic::EYE_SLASH, "Ocultar coluna", None) {
        actions.push(GridAction::HideColumn(tc));
        ui.close();
    }
    ui.separator();
    if menu_item(ui, ic::CLIPBOARD_TEXT, "Copiar nome da coluna", None) {
        ui.ctx().copy_text(g.table.columns[tc].name.clone());
        actions.push(GridAction::Toast("Nome copiado".to_owned()));
        ui.close();
    }
    if menu_item(ui, ic::COPY, "Copiar coluna inteira", None) {
        let n = g.view.len();
        if n > MAX_COPY_CELLS {
            actions.push(GridAction::Toast("Coluna grande demais para copiar. Use Exportar.".to_owned()));
        } else if n > 0 {
            let text = selection_tsv(g.table, g.view, 0..=n - 1, &[tc], true);
            ui.ctx().copy_text(text);
            actions.push(GridAction::Toast(format!("{} valores copiados", format::int(n))));
        }
        ui.close();
    }
}

/// Ajusta a largura ao conteúdo das linhas visíveis.
fn autofit(ui: &Ui, st: &mut GridState, g: &GridCtx<'_>, tc: usize, lay: &Layout) {
    let column = &g.table.columns[tc];
    let n_rows = g.view.len();
    let first = (st.scroll_y / lay.row_h as f64) as usize;
    let visible = (lay.body.height() / lay.row_h).ceil() as usize + 1;
    let font = cell_font(column.kind);
    let mut w = ui.fonts_mut(|f| {
        f.layout_no_wrap(column.name.clone(), header_font(), Color32::WHITE)
            .size()
            .x
    }) + 2.0 * PAD_X
        + 42.0;
    for i in first..(first + visible).min(n_rows) {
        let text = display_text(column.get(g.view.row(i))).into_owned();
        let tw = ui.fonts_mut(|f| f.layout_no_wrap(text, font.clone(), Color32::WHITE).size().x);
        w = w.max(tw + 2.0 * PAD_X + 4.0);
    }
    if let Some(slot) = st.widths.get_mut(tc) {
        *slot = w.clamp(MIN_COL_W, 900.0).round();
    }
}

#[allow(clippy::too_many_arguments)]
fn paint(
    ui: &Ui,
    st: &GridState,
    g: &GridCtx<'_>,
    lay: &Layout,
    p: &Palette,
    show_v: bool,
    show_h: bool,
    v_resp: &Option<Response>,
    h_resp: &Option<Response>,
    v_handle: (f32, f32),
    h_handle: (f32, f32),
) {
    let n_rows = g.view.len();
    let ncols = g.cols.len();
    let row_h = lay.row_h;
    let painter = ui.painter_at(lay.rect);
    painter.rect_filled(lay.rect, 0.0, p.bg);

    let first_row = (st.scroll_y / row_h as f64).floor().max(0.0) as usize;
    let last_row = (((st.scroll_y + lay.body.height() as f64) / row_h as f64).ceil() as usize).min(n_rows);
    let first_col = lay.xs.partition_point(|&x| x <= st.scroll_x).saturating_sub(1);
    let last_col = lay.xs.partition_point(|&x| x < st.scroll_x + lay.body.width()).min(ncols);
    let row_y = |r: usize| lay.body.min.y + (r as f64 * row_h as f64 - st.scroll_y) as f32;
    let col_x = |ci: usize| lay.body.min.x + lay.xs[ci] - st.scroll_x;

    let body_p = painter.with_clip_rect(lay.body);
    let hover_row = ui
        .input(|i| i.pointer.hover_pos())
        .filter(|pos| (lay.body.contains(*pos) || lay.gutter.contains(*pos)) && ui.rect_contains_pointer(lay.rect))
        .map(|pos| ((pos.y - lay.body.min.y) as f64 + st.scroll_y) / row_h as f64)
        .filter(|r| *r >= 0.0)
        .map(|r| r as usize)
        .filter(|&r| r < n_rows);
    let selection = st.selection(g.cols);
    let cursor_ci = st.cursor.and_then(|c| g.cols.iter().position(|&x| x == c.col));

    // Fundos de linha.
    for r in first_row..last_row {
        let y = row_y(r);
        let row_rect = Rect::from_min_size(pos2(lay.body.min.x, y), vec2(lay.body.width(), row_h));
        if g.zebra && r % 2 == 1 {
            body_p.rect_filled(row_rect, 0.0, p.zebra);
        }
        if Some(r) == hover_row {
            body_p.rect_filled(row_rect, 0.0, p.row_hover);
        }
    }

    // Seleção.
    if let Some((rr, cr)) = &selection {
        let x0 = col_x(*cr.start());
        let x1 = col_x(cr.end() + 1);
        let y0 = row_y(*rr.start()).max(lay.body.min.y - 4.0);
        let y1 = row_y(rr.end() + 1).min(lay.body.max.y + 4.0);
        if y1 > y0 {
            let sel = Rect::from_min_max(pos2(x0, y0), pos2(x1, y1));
            let multi = rr.start() != rr.end() || cr.start() != cr.end();
            if multi {
                body_p.rect_filled(sel, 0.0, p.selection);
                body_p.rect_stroke(sel, 0.0, Stroke::new(1.0, p.selection_border), StrokeKind::Inside);
            } else {
                body_p.rect_filled(sel, 0.0, p.accent_faint);
            }
        }
        // Linha do cursor um pouco mais destacada.
        if let Some(c) = st.cursor {
            let y = row_y(c.row);
            let row_rect = Rect::from_min_size(pos2(lay.body.min.x, y), vec2(lay.body.width(), row_h));
            body_p.rect_filled(row_rect, 0.0, p.accent_faint.gamma_multiply(0.5));
        }
    }

    // Linhas de grade.
    let line = Stroke::new(1.0, p.grid_line);
    for r in first_row..=last_row {
        let y = painter.round_to_pixel_center(row_y(r) - 0.5);
        if y > lay.body.min.y && y < lay.body.max.y {
            body_p.hline(lay.body.x_range(), y, line);
        }
    }
    let content_bottom = row_y(n_rows).min(lay.body.max.y);
    for ci in (first_col + 1)..=last_col {
        let x = painter.round_to_pixel_center(col_x(ci) - 0.5);
        body_p.vline(x, lay.body.min.y..=content_bottom, line);
    }

    // Texto das células.
    for r in first_row..last_row {
        let data_row = g.view.row(r);
        let y = row_y(r);
        for ci in first_col..last_col {
            let tc = g.cols[ci];
            let column = &g.table.columns[tc];
            let raw = column.get(data_row);
            if raw.is_empty() {
                continue;
            }
            let x0 = col_x(ci);
            let w = lay.xs[ci + 1] - lay.xs[ci];
            let inner_w = (w - 2.0 * PAD_X).max(6.0);
            let text = display_text(raw);
            let in_sel = selection
                .as_ref()
                .is_some_and(|(rr, cr)| rr.contains(&r) && cr.contains(&ci));
            let color = if column.kind == ColumnKind::Empty || (!in_sel && raw.trim().is_empty()) {
                p.text_faint
            } else {
                p.text
            };
            let job = cell_job(&text, cell_font(column.kind), color, inner_w, g.needle, p.search_hit);
            let galley = painter.layout_job(job);
            let tx = if column.kind.align_right() {
                x0 + w - PAD_X - galley.size().x
            } else {
                x0 + PAD_X
            };
            let ty = y + (row_h - galley.size().y) / 2.0;
            let cell_rect = Rect::from_min_size(pos2(x0, y), vec2(w, row_h)).intersect(lay.body);
            if w < 2.0 * PAD_X + 20.0 {
                painter.with_clip_rect(cell_rect).galley(pos2(tx, ty), galley, color);
            } else {
                body_p.galley(pos2(tx, ty), galley, color);
            }
        }
    }

    // Cursor.
    if let (Some(c), Some(ci)) = (st.cursor, cursor_ci) {
        let y = row_y(c.row);
        let cell = Rect::from_min_size(pos2(col_x(ci), y), vec2(lay.xs[ci + 1] - lay.xs[ci], row_h));
        if cell.intersects(lay.body) {
            body_p.rect_stroke(cell.shrink(0.5), CornerRadius::same(3), Stroke::new(2.0, p.accent), StrokeKind::Inside);
        }
    }

    // Sombras quando rolado.
    if st.scroll_y > 0.5 {
        let r = Rect::from_min_size(lay.body.min - vec2(lay.gutter.width(), 0.0), vec2(lay.rect.width(), 10.0));
        gradient(&painter, r, p.shadow.gamma_multiply(0.55), true);
    }
    if st.scroll_x > 0.5 {
        let r = Rect::from_min_size(pos2(lay.body.min.x, lay.rect.min.y), vec2(10.0, lay.rect.height()));
        gradient(&painter, r, p.shadow.gamma_multiply(0.5), false);
    }

    // Cabeçalho.
    let head_p = painter.with_clip_rect(lay.header);
    head_p.rect_filled(lay.header, 0.0, p.surface);
    let hover_pos = ui.input(|i| i.pointer.hover_pos());
    for ci in first_col..last_col {
        let tc = g.cols[ci];
        let column = &g.table.columns[tc];
        let x0 = lay.header.min.x + lay.xs[ci] - st.scroll_x;
        let w = lay.xs[ci + 1] - lay.xs[ci];
        let cell = Rect::from_min_size(pos2(x0, lay.header.min.y), vec2(w, HEADER_H));
        let hovered = hover_pos.is_some_and(|pos| cell.contains(pos) && lay.header.contains(pos))
            && st.drag == Drag::None
            && ui.rect_contains_pointer(lay.header);
        let selected = cursor_ci == Some(ci) || selection.as_ref().is_some_and(|(_, cr)| cr.contains(&ci));
        if hovered {
            head_p.rect_filled(cell, 0.0, p.surface_hover);
        }
        if selected {
            head_p.rect_filled(cell, 0.0, p.accent_faint);
            head_p.hline(cell.x_range(), cell.max.y - 1.0, Stroke::new(2.0, p.accent));
        }
        let sorted = g.sort.filter(|s| s.col == tc);
        let filtered = g.filtered_cols.contains(&tc);

        // Ícone do tipo.
        let icon_x = cell.min.x + PAD_X;
        head_p.text(
            pos2(icon_x, cell.center().y),
            Align2::LEFT_CENTER,
            theme::kind_icon(column.kind),
            theme::icon(14.0),
            p.kind_color(column.kind),
        );
        // Indicadores à direita.
        let mut right = cell.max.x - PAD_X + 2.0;
        if let Some(s) = sorted {
            let icon = if s.descending { ic::ARROW_DOWN } else { ic::ARROW_UP };
            let r = head_p.text(pos2(right, cell.center().y), Align2::RIGHT_CENTER, icon, theme::icon(13.0), p.accent);
            right = r.min.x - 3.0;
        }
        if filtered {
            let r = head_p.text(pos2(right, cell.center().y), Align2::RIGHT_CENTER, ic::FUNNEL, theme::icon(13.0), p.accent);
            right = r.min.x - 3.0;
        }
        let name_x = icon_x + 21.0;
        let max_w = (right - name_x - 2.0).max(4.0);
        let job = LayoutJob {
            wrap: TextWrapping::truncate_at_width(max_w),
            ..LayoutJob::single_section(
                column.name.clone(),
                TextFormat {
                    font_id: header_font(),
                    color: if selected || sorted.is_some() { p.text } else { p.text_muted.lerp_to_gamma(p.text, 0.45) },
                    valign: Align::Center,
                    ..Default::default()
                },
            )
        };
        let galley = head_p.layout_job(job);
        let ty = cell.center().y - galley.size().y / 2.0;
        head_p.galley(pos2(name_x, ty), galley, p.text);
        // Divisória.
        let x = painter.round_to_pixel_center(cell.max.x - 0.5);
        head_p.vline(x, (cell.min.y + 9.0)..=(cell.max.y - 9.0), Stroke::new(1.0, p.border));
    }
    head_p.hline(lay.header.x_range(), painter.round_to_pixel_center(lay.header.max.y - 0.5), Stroke::new(1.0, p.border));
    if let Drag::Resizing { col, .. } = st.drag
        && let Some(ci) = g.cols.iter().position(|&x| x == col)
    {
        let x = lay.header.min.x + lay.xs[ci + 1] - st.scroll_x;
        painter.vline(x, lay.rect.y_range(), Stroke::new(2.0, p.accent));
    }

    // Numeração das linhas.
    let gut_p = painter.with_clip_rect(lay.gutter);
    gut_p.rect_filled(lay.gutter, 0.0, p.panel);
    let sel_rows = selection.as_ref().map(|(rr, _)| rr.clone());
    for r in first_row..last_row {
        let y = row_y(r);
        let row_rect = Rect::from_min_size(pos2(lay.gutter.min.x, y), vec2(lay.gutter.width(), row_h));
        let selected = sel_rows.as_ref().is_some_and(|rr| rr.contains(&r));
        if selected {
            gut_p.rect_filled(row_rect, 0.0, p.accent_faint);
        } else if Some(r) == hover_row {
            gut_p.rect_filled(row_rect, 0.0, p.row_hover);
        }
        let n = g.view.row(r) + 1;
        gut_p.text(
            pos2(row_rect.max.x - 10.0, row_rect.center().y),
            Align2::RIGHT_CENTER,
            n.to_string(),
            theme::mono(11.5),
            if selected { p.accent } else { p.text_faint },
        );
    }
    let gx = painter.round_to_pixel_center(lay.gutter.max.x - 0.5);
    painter.vline(gx, lay.rect.y_range(), Stroke::new(1.0, p.border));

    // Canto.
    painter.rect_filled(lay.corner, 0.0, p.surface);
    let all_selected = selection.as_ref().is_some_and(|(rr, cr)| {
        *rr.start() == 0 && *rr.end() + 1 == n_rows && *cr.start() == 0 && *cr.end() + 1 == ncols
    });
    painter.text(
        lay.corner.center(),
        Align2::CENTER_CENTER,
        ic::SELECTION_ALL,
        theme::icon(15.0),
        if all_selected { p.accent } else { p.text_faint },
    );
    painter.hline(lay.corner.x_range(), painter.round_to_pixel_center(lay.corner.max.y - 0.5), Stroke::new(1.0, p.border));

    // Barras de rolagem (sobrepostas, finas, engrossam ao passar o mouse).
    let bar = |resp: &Option<Response>, dragging: bool, id: &str| -> f32 {
        let active = dragging || resp.as_ref().is_some_and(|r| r.hovered());
        ui.ctx().animate_bool_with_time(g.id.with(id), active, 0.15)
    };
    if show_v {
        let t = bar(v_resp, matches!(st.drag, Drag::VBar { .. }), "vbar_anim");
        let w = 5.0 + 4.0 * t;
        let (start, len) = v_handle;
        let r = Rect::from_min_max(
            pos2(lay.body.max.x - w - 3.0, start + 2.0),
            pos2(lay.body.max.x - 3.0, start + len - 2.0),
        );
        let color = p.text_faint.gamma_multiply(0.45 + 0.4 * t);
        painter.rect_filled(r, CornerRadius::same(5), color);
    }
    if show_h {
        let t = bar(h_resp, matches!(st.drag, Drag::HBar { .. }), "hbar_anim");
        let h = 5.0 + 4.0 * t;
        let (start, len) = h_handle;
        let r = Rect::from_min_max(
            pos2(start + 2.0, lay.body.max.y - h - 3.0),
            pos2(start + len - 2.0, lay.body.max.y - 3.0),
        );
        let color = p.text_faint.gamma_multiply(0.45 + 0.4 * t);
        painter.rect_filled(r, CornerRadius::same(5), color);
    }
}

fn empty_state(ui: &mut Ui, lay: &Layout, p: &Palette, g: &GridCtx<'_>, actions: &mut Vec<GridAction>) {
    let center = lay.body.center();
    let painter = ui.painter_at(lay.body);
    painter.text(
        center - vec2(0.0, 34.0),
        Align2::CENTER_CENTER,
        ic::MAGNIFYING_GLASS,
        theme::icon(30.0),
        p.text_faint,
    );
    let msg = if g.table.columns.is_empty() {
        "O arquivo está vazio"
    } else if g.table.rows == 0 {
        "Este arquivo não tem linhas de dados"
    } else {
        "Nenhuma linha corresponde à busca ou aos filtros"
    };
    painter.text(center, Align2::CENTER_CENTER, msg, theme::medium(14.5), p.text_muted);
    if g.table.rows > 0 {
        let btn_rect = Rect::from_center_size(center + vec2(0.0, 38.0), vec2(150.0, 30.0));
        let resp = ui.put(
            btn_rect,
            egui::Button::new((theme::icon_text(ic::BROOM, 15.0), "Limpar filtros")).gap(8.0),
        );
        if resp.clicked() {
            actions.push(GridAction::ClearFilters);
        }
    }
}
