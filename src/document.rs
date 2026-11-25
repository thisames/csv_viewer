//! Um arquivo aberto (uma aba): dados, visão atual, colunas e tarefas em
//! segundo plano.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use crate::data::loader::{self, LoadOptions, Loaded, Progress};
use crate::data::query::{self, Filter, Sort, View, ViewSpec};
use crate::data::stats::{self, ColumnStats, SelectionSummary};
use crate::data::table::{SourceInfo, Table};
use crate::data::text::Needle;
use crate::ui::grid::GridState;

pub enum DocState {
    Loading {
        progress: Arc<Progress>,
        rx: mpsc::Receiver<Result<Loaded, String>>,
        started: Instant,
    },
    Ready,
    Failed(String),
}

struct ViewJob {
    spec: ViewSpec,
    rx: mpsc::Receiver<Option<View>>,
    started: Instant,
}

struct StatsJob {
    key: (u64, usize),
    rx: mpsc::Receiver<ColumnStats>,
}

pub struct Document {
    pub id: u64,
    pub path: PathBuf,
    pub title: String,
    pub state: DocState,
    pub opts: LoadOptions,
    pub table: Option<Arc<Table>>,
    pub info: Option<SourceInfo>,

    /// O que o usuário pediu (busca, filtros, ordem).
    pub spec: ViewSpec,
    /// Visão em exibição e a especificação que a gerou.
    pub view: Arc<View>,
    pub view_spec: ViewSpec,
    /// Muda a cada visão nova (invalida estatísticas).
    pub view_version: u64,
    generation: Arc<AtomicU64>,
    view_job: Option<ViewJob>,
    pub needle: Needle,

    pub search_input: String,
    search_edited: Option<Instant>,

    pub hidden: Vec<bool>,
    pub order: Vec<usize>,
    pub column_query: String,
    /// Pedido para focar o campo de filtro de uma coluna na barra lateral.
    pub focus_column_filter: bool,
    /// Texto do campo "contém…" por coluna, na barra lateral.
    pub contains_input: String,
    pub min_input: String,
    pub max_input: String,
    pub filter_input_col: Option<usize>,

    pub stats: Option<Arc<ColumnStats>>,
    stats_key: Option<(u64, usize)>,
    stats_job: Option<StatsJob>,

    pub selection_summary: Option<(SelectionKey, SelectionSummary)>,

    /// Acabou de carregar com sucesso (o app usa para a lista de recentes).
    pub just_loaded: bool,

    pub grid: GridState,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SelectionKey {
    pub view_version: u64,
    pub rows: (usize, usize),
    pub cols: Vec<usize>,
}

impl Document {
    pub fn open(id: u64, path: PathBuf, opts: LoadOptions, ctx: &egui::Context) -> Self {
        let title = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        let mut doc = Self {
            id,
            path,
            title,
            state: DocState::Failed(String::new()),
            opts,
            table: None,
            info: None,
            spec: ViewSpec::default(),
            view: Arc::new(View::All(0)),
            view_spec: ViewSpec::default(),
            view_version: 0,
            generation: Arc::new(AtomicU64::new(0)),
            view_job: None,
            needle: Needle::default(),
            search_input: String::new(),
            search_edited: None,
            hidden: Vec::new(),
            order: Vec::new(),
            column_query: String::new(),
            focus_column_filter: false,
            contains_input: String::new(),
            min_input: String::new(),
            max_input: String::new(),
            filter_input_col: None,
            stats: None,
            stats_key: None,
            stats_job: None,
            selection_summary: None,
            just_loaded: false,
            grid: GridState::default(),
        };
        doc.start_loading(ctx);
        doc
    }

    pub fn start_loading(&mut self, ctx: &egui::Context) {
        let progress = Arc::new(Progress::default());
        let (tx, rx) = mpsc::channel();
        let path = self.path.clone();
        let opts = self.opts.clone();
        let prog = progress.clone();
        let ctx = ctx.clone();
        std::thread::Builder::new()
            .name("carregar-arquivo".into())
            .spawn(move || {
                let result = loader::load(&path, &opts, &prog);
                let _ = tx.send(result);
                ctx.request_repaint();
            })
            .expect("não foi possível criar a thread de leitura");
        self.state = DocState::Loading {
            progress,
            rx,
            started: Instant::now(),
        };
    }

    /// Recarrega com outras opções (cabeçalho, separador, codificação, aba).
    pub fn reload(&mut self, opts: LoadOptions, ctx: &egui::Context) {
        self.cancel_loading();
        let keep_widths = opts.sheet == self.opts.sheet
            && opts.has_header == self.opts.has_header
            && opts.delimiter == self.opts.delimiter;
        self.opts = opts;
        if !keep_widths {
            self.grid.widths.clear();
        }
        self.start_loading(ctx);
    }

    pub fn cancel_loading(&mut self) {
        if let DocState::Loading { progress, .. } = &self.state {
            progress.cancel.store(true, Ordering::Relaxed);
        }
    }

    pub fn is_ready(&self) -> bool {
        matches!(self.state, DocState::Ready) && self.table.is_some()
    }

    pub fn is_computing(&self) -> bool {
        self.view_job
            .as_ref()
            .is_some_and(|j| j.started.elapsed() > Duration::from_millis(120))
    }

    pub fn same_path(&self, other: &Path) -> bool {
        let a = std::fs::canonicalize(&self.path).unwrap_or_else(|_| self.path.clone());
        let b = std::fs::canonicalize(other).unwrap_or_else(|_| other.to_path_buf());
        a == b
    }

    /// Colunas visíveis na ordem de exibição.
    pub fn visible_cols(&self) -> Vec<usize> {
        self.order
            .iter()
            .copied()
            .filter(|&c| !self.hidden.get(c).copied().unwrap_or(false))
            .collect()
    }

    pub fn selected_col(&self) -> Option<usize> {
        self.grid.cursor.map(|c| c.col)
    }

    pub fn set_sort(&mut self, sort: Option<Sort>) {
        self.spec.sort = sort;
    }

    pub fn add_filter(&mut self, filter: Filter) {
        // Substitui um filtro idêntico em vez de duplicar.
        if !self.spec.filters.contains(&filter) {
            self.spec.filters.push(filter);
        }
    }

    pub fn remove_filter(&mut self, i: usize) {
        if i < self.spec.filters.len() {
            self.spec.filters.remove(i);
        }
    }

    pub fn clear_filters(&mut self) {
        self.spec.filters.clear();
        self.search_input.clear();
        self.spec.search.clear();
        self.search_edited = None;
    }

    pub fn set_hidden(&mut self, col: usize, hidden: bool) {
        if let Some(h) = self.hidden.get_mut(col) {
            *h = hidden;
        }
        if hidden && self.grid.cursor.is_some_and(|c| c.col == col) {
            // Move o cursor para uma coluna visível.
            let vis = self.visible_cols();
            if let (Some(c), Some(&first)) = (self.grid.cursor, vis.first()) {
                self.grid.cursor = Some(crate::ui::grid::CellPos { row: c.row, col: first });
                self.grid.anchor = self.grid.cursor;
            }
        }
    }

    /// Avança o estado: recebe resultados das threads e dispara novas tarefas.
    pub fn poll(&mut self, ctx: &egui::Context) {
        // 1) Carregamento.
        if let DocState::Loading { rx, progress, .. } = &self.state {
            match rx.try_recv() {
                Ok(Ok(loaded)) => self.on_loaded(loaded),
                Ok(Err(e)) => self.state = DocState::Failed(e),
                Err(mpsc::TryRecvError::Empty) => {
                    let _ = progress;
                    ctx.request_repaint_after(Duration::from_millis(60));
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.state = DocState::Failed("A leitura foi interrompida.".to_owned());
                }
            }
        }
        let Some(table) = self.table.clone() else {
            return;
        };
        if !matches!(self.state, DocState::Ready) {
            return;
        }

        // 2) Busca com pequeno atraso enquanto digita (arquivos grandes).
        if self.search_input != self.spec.search {
            let delay = if table.rows > 200_000 {
                Duration::from_millis(220)
            } else {
                Duration::ZERO
            };
            let edited = *self.search_edited.get_or_insert_with(Instant::now);
            if edited.elapsed() >= delay {
                self.spec.search = self.search_input.clone();
                self.search_edited = None;
            } else {
                ctx.request_repaint_after(delay.saturating_sub(edited.elapsed()));
            }
        } else {
            self.search_edited = None;
        }
        self.spec.search_cols = self.visible_cols();
        if self.spec.search.trim().is_empty() {
            // As colunas de busca só importam quando há busca.
            self.spec.search_cols = self.view_spec.search_cols.clone();
        }

        // 3) Resultado da visão.
        if let Some(job) = &self.view_job {
            match job.rx.try_recv() {
                Ok(Some(view)) => {
                    let job = self.view_job.take().expect("job existe");
                    self.apply_view(view, job.spec);
                }
                Ok(None) | Err(mpsc::TryRecvError::Disconnected) => self.view_job = None,
                Err(mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(Duration::from_millis(30));
                }
            }
        }

        // 4) Nova visão, se o pedido mudou.
        let pending_spec = self.view_job.as_ref().map(|j| &j.spec);
        if self.spec != self.view_spec && pending_spec != Some(&self.spec) {
            self.start_view_job(&table, ctx);
        }

        // 5) Estatísticas da coluna selecionada.
        if let Some(col) = self.selected_col().filter(|&c| c < table.columns.len()) {
            let key = (self.view_version, col);
            if let Some(job) = &self.stats_job
                && let Ok(stats) = job.rx.try_recv()
            {
                let key_done = job.key;
                self.stats_job = None;
                self.stats = Some(Arc::new(stats));
                self.stats_key = Some(key_done);
            }
            if self.stats_key != Some(key) && self.stats_job.as_ref().map(|j| j.key) != Some(key) {
                let (tx, rx) = mpsc::channel();
                let view = self.view.clone();
                let t = table.clone();
                let ctx = ctx.clone();
                rayon::spawn(move || {
                    let s = stats::compute(&t, &view, col);
                    let _ = tx.send(s);
                    ctx.request_repaint();
                });
                self.stats_job = Some(StatsJob { key, rx });
            }
        }

        // 6) Resumo da seleção (soma/média na barra de status).
        self.update_selection_summary(&table);
    }

    pub fn stats_for(&self, col: usize) -> Option<&ColumnStats> {
        self.stats.as_deref().filter(|s| s.col == col)
    }

    fn on_loaded(&mut self, loaded: Loaded) {
        let Loaded { table, info } = loaded;
        let ncols = table.columns.len();
        let same_shape = self.order.len() == ncols && self.hidden.len() == ncols;
        if !same_shape {
            self.order = (0..ncols).collect();
            self.hidden = vec![false; ncols];
            self.spec.filters.clear();
            self.spec.sort = None;
            self.grid = GridState::default();
        } else {
            self.spec.filters.retain(|f| f.col < ncols);
        }
        self.view = Arc::new(View::All(table.rows));
        self.view_spec = ViewSpec {
            search_cols: self.visible_cols(),
            ..Default::default()
        };
        self.view_version += 1;
        self.stats = None;
        self.stats_key = None;
        self.stats_job = None;
        self.selection_summary = None;
        if let Some(c) = self.grid.cursor
            && (c.row >= table.rows || c.col >= ncols)
        {
            self.grid.clear_selection();
        }
        if self.grid.cursor.is_none() && table.rows > 0 && ncols > 0 {
            self.grid.cursor = Some(crate::ui::grid::CellPos { row: 0, col: 0 });
            self.grid.anchor = self.grid.cursor;
        }
        self.table = Some(Arc::new(table));
        self.info = Some(info);
        self.state = DocState::Ready;
        self.just_loaded = true;
    }

    fn start_view_job(&mut self, table: &Arc<Table>, ctx: &egui::Context) {
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let spec = self.spec.clone();
        let (tx, rx) = mpsc::channel();
        let t = table.clone();
        let g = self.generation.clone();
        let s = spec.clone();
        let ctx = ctx.clone();
        rayon::spawn(move || {
            let view = query::compute_view(&t, &s, &g, generation);
            let _ = tx.send(view);
            ctx.request_repaint();
        });
        self.view_job = Some(ViewJob {
            spec,
            rx,
            started: Instant::now(),
        });
    }

    fn apply_view(&mut self, view: View, spec: ViewSpec) {
        let old = self.view.clone();
        let changed = spec.sort != self.view_spec.sort
            || spec.filters != self.view_spec.filters
            || spec.search != self.view_spec.search;
        if changed {
            // Nova ordem/filtro: volta ao topo, mantendo a coluna do cursor.
            self.grid.scroll_y = 0.0;
            match (self.grid.cursor, view.is_empty()) {
                (_, true) => self.grid.clear_selection(),
                (Some(c), false) => {
                    self.grid.cursor = Some(crate::ui::grid::CellPos { row: 0, col: c.col });
                    self.grid.anchor = self.grid.cursor;
                }
                (None, false) => {}
            }
        } else {
            self.grid.remap(&old, &view);
        }
        self.needle = Needle::new(&spec.search);
        self.view = Arc::new(view);
        self.view_spec = spec;
        self.view_version += 1;
    }

    fn update_selection_summary(&mut self, table: &Table) {
        let cols = self.visible_cols();
        let Some((rows, cr)) = self.grid.selection(&cols) else {
            self.selection_summary = None;
            return;
        };
        let sel_cols: Vec<usize> = cols[cr].to_vec();
        let key = SelectionKey {
            view_version: self.view_version,
            rows: (*rows.start(), *rows.end()),
            cols: sel_cols.clone(),
        };
        if self.selection_summary.as_ref().is_some_and(|(k, _)| *k == key) {
            return;
        }
        let cells = (rows.end() - rows.start() + 1) * sel_cols.len();
        if cells <= 1 || rows.end() >= &self.view.len() {
            self.selection_summary = None;
            return;
        }
        let summary = if cells <= 3_000_000 {
            stats::summarize_selection(table, &self.view, rows, &sel_cols)
        } else {
            SelectionSummary {
                cells,
                ..Default::default()
            }
        };
        self.selection_summary = Some((key, summary));
    }
}
