//! Filtros, busca e ordenação. Produz a "visão": a lista de linhas visíveis,
//! na ordem em que devem aparecer.

use std::cmp::Ordering;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

use rayon::prelude::*;

use super::table::Table;
use super::text::{Needle, cmp_text};

#[derive(Clone, Debug, PartialEq)]
pub enum FilterOp {
    Contains(String),
    Equals(String),
    NotEquals(String),
    Empty,
    NotEmpty,
    AtLeast(f64),
    AtMost(f64),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Filter {
    pub col: usize,
    pub op: FilterOp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sort {
    pub col: usize,
    pub descending: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ViewSpec {
    pub search: String,
    /// Colunas onde a busca procura (as visíveis).
    pub search_cols: Vec<usize>,
    pub filters: Vec<Filter>,
    pub sort: Option<Sort>,
}

impl ViewSpec {
    pub fn is_identity(&self) -> bool {
        self.search.trim().is_empty() && self.filters.is_empty() && self.sort.is_none()
    }
}

/// Linhas visíveis.
#[derive(Clone, Debug)]
pub enum View {
    /// Todas as linhas, na ordem original.
    All(usize),
    Rows(Vec<u32>),
}

impl View {
    #[inline]
    pub fn len(&self) -> usize {
        match self {
            Self::All(n) => *n,
            Self::Rows(r) => r.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Linha da tabela que aparece na posição `i`.
    #[inline]
    pub fn row(&self, i: usize) -> usize {
        match self {
            Self::All(_) => i,
            Self::Rows(r) => r[i] as usize,
        }
    }

    pub fn position_of(&self, data_row: usize) -> Option<usize> {
        match self {
            Self::All(n) => (data_row < *n).then_some(data_row),
            Self::Rows(r) => r.par_iter().position_any(|&x| x as usize == data_row),
        }
    }
}

enum Prepared<'a> {
    Contains(Needle),
    Equals(&'a str),
    NotEquals(&'a str),
    Empty,
    NotEmpty,
    AtLeast(f64),
    AtMost(f64),
}

/// Calcula a visão. Devolve `None` se `generation` mudou no meio do caminho
/// (o usuário já pediu outra coisa).
pub fn compute_view(
    table: &Table,
    spec: &ViewSpec,
    generation: &AtomicU64,
    my_generation: u64,
) -> Option<View> {
    if spec.is_identity() {
        return Some(View::All(table.rows));
    }
    let stale = || generation.load(AtomicOrdering::Relaxed) != my_generation;

    let filters: Vec<(usize, Prepared<'_>)> = spec
        .filters
        .iter()
        .filter(|f| f.col < table.columns.len())
        .map(|f| {
            let p = match &f.op {
                FilterOp::Contains(s) => Prepared::Contains(Needle::new(s)),
                FilterOp::Equals(s) => Prepared::Equals(s.as_str()),
                FilterOp::NotEquals(s) => Prepared::NotEquals(s.as_str()),
                FilterOp::Empty => Prepared::Empty,
                FilterOp::NotEmpty => Prepared::NotEmpty,
                FilterOp::AtLeast(v) => Prepared::AtLeast(*v),
                FilterOp::AtMost(v) => Prepared::AtMost(*v),
            };
            (f.col, p)
        })
        .collect();

    let needle = Needle::new(&spec.search);
    let search_cols: Vec<usize> = spec
        .search_cols
        .iter()
        .copied()
        .filter(|&c| c < table.columns.len())
        .collect();

    let row_matches = |r: usize| -> bool {
        for (col, f) in &filters {
            let column = &table.columns[*col];
            let ok = match f {
                Prepared::Contains(n) => n.matches(column.get(r)),
                Prepared::Equals(s) => column.get(r) == *s,
                Prepared::NotEquals(s) => column.get(r) != *s,
                Prepared::Empty => column.get(r).trim().is_empty(),
                Prepared::NotEmpty => !column.get(r).trim().is_empty(),
                Prepared::AtLeast(v) => column.key(r).is_some_and(|k| k >= *v),
                Prepared::AtMost(v) => column.key(r).is_some_and(|k| k <= *v),
            };
            if !ok {
                return false;
            }
        }
        if !needle.is_empty() {
            return search_cols
                .iter()
                .any(|&c| needle.matches(table.columns[c].get(r)));
        }
        true
    };

    let filtering = !filters.is_empty() || !needle.is_empty();
    let mut rows: Vec<u32> = if filtering {
        (0..table.rows as u32)
            .into_par_iter()
            .with_min_len(4096)
            .filter(|&r| {
                // Checagem barata; se a geração mudou o resultado será descartado.
                if r % 65_536 == 0 && stale() {
                    return false;
                }
                row_matches(r as usize)
            })
            .collect()
    } else {
        (0..table.rows as u32).collect()
    };
    if stale() {
        return None;
    }

    if let Some(sort) = spec.sort
        && sort.col < table.columns.len()
    {
        sort_rows(table, &mut rows, sort);
        if stale() {
            return None;
        }
    }

    if !filtering && spec.sort.is_none() {
        return Some(View::All(table.rows));
    }
    Some(View::Rows(rows))
}

/// Ordena de forma estável. Vazios (e valores inválidos) vão sempre para o fim.
pub fn sort_rows(table: &Table, rows: &mut [u32], sort: Sort) {
    let column = &table.columns[sort.col];
    let desc = sort.descending;
    if let Some(keys) = &column.keys {
        rows.par_sort_by(|&a, &b| {
            let (ka, kb) = (keys[a as usize], keys[b as usize]);
            match (ka.is_nan(), kb.is_nan()) {
                (true, true) => Ordering::Equal,
                (true, false) => Ordering::Greater,
                (false, true) => Ordering::Less,
                (false, false) => {
                    let o = ka.partial_cmp(&kb).unwrap_or(Ordering::Equal);
                    if desc { o.reverse() } else { o }
                }
            }
        });
    } else {
        rows.par_sort_by(|&a, &b| {
            let (sa, sb) = (column.get(a as usize), column.get(b as usize));
            match (sa.is_empty(), sb.is_empty()) {
                (true, true) => Ordering::Equal,
                (true, false) => Ordering::Greater,
                (false, true) => Ordering::Less,
                (false, false) => {
                    let o = cmp_text(sa, sb);
                    if desc { o.reverse() } else { o }
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::table::ColumnBuilder;

    fn table(cols: &[(&str, &[&str])]) -> Table {
        let rows = cols[0].1.len();
        let columns = cols
            .iter()
            .map(|(name, vals)| {
                let mut b = ColumnBuilder::new(name.to_string());
                for v in *vals {
                    b.push(v).unwrap();
                }
                let mut c = b.finish();
                let inf = crate::data::infer::infer_column(
                    rows,
                    |r| vals[r],
                    crate::data::infer::DecimalSep::Comma,
                );
                c.kind = inf.kind;
                c.keys = inf.keys;
                c
            })
            .collect();
        Table { columns, rows }
    }

    #[test]
    fn filtra_busca_e_ordena() {
        let t = table(&[
            ("cidade", &["São Paulo", "Rio", "santos", "Recife", ""]),
            ("valor", &["10,5", "2", "", "100", "7"]),
        ]);
        let g = AtomicU64::new(1);
        let spec = ViewSpec {
            search: "sao".into(),
            search_cols: vec![0, 1],
            ..Default::default()
        };
        let v = compute_view(&t, &spec, &g, 1).unwrap();
        assert_eq!(v.len(), 1);
        assert_eq!(v.row(0), 0);

        let spec = ViewSpec {
            sort: Some(Sort {
                col: 1,
                descending: true,
            }),
            ..Default::default()
        };
        let v = compute_view(&t, &spec, &g, 1).unwrap();
        let order: Vec<usize> = (0..v.len()).map(|i| v.row(i)).collect();
        assert_eq!(order, vec![3, 0, 4, 1, 2]); // vazio por último

        let spec = ViewSpec {
            sort: Some(Sort {
                col: 0,
                descending: false,
            }),
            filters: vec![Filter {
                col: 1,
                op: FilterOp::AtLeast(5.0),
            }],
            ..Default::default()
        };
        let v = compute_view(&t, &spec, &g, 1).unwrap();
        let order: Vec<usize> = (0..v.len()).map(|i| v.row(i)).collect();
        assert_eq!(order, vec![3, 0, 4]); // Recife, São Paulo, (vazio)
    }

    #[test]
    fn geracao_antiga_e_descartada() {
        let t = table(&[("a", &["1", "2"])]);
        let g = AtomicU64::new(2);
        let spec = ViewSpec {
            search: "1".into(),
            search_cols: vec![0],
            ..Default::default()
        };
        assert!(compute_view(&t, &spec, &g, 1).is_none());
    }
}
