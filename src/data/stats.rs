//! Estatísticas de uma coluna (sobre as linhas visíveis).

use rustc_hash::FxHashMap;

use super::query::View;
use super::table::{ColumnKind, Table};

pub const HISTOGRAM_BINS: usize = 28;
const DISTINCT_CAP: usize = 250_000;

#[derive(Clone, Debug)]
pub struct NumericStats {
    pub count: usize,
    pub min: f64,
    pub max: f64,
    pub sum: f64,
    pub mean: f64,
    pub median: f64,
    pub std_dev: f64,
    pub histogram: Vec<u32>,
}

#[derive(Clone, Debug)]
pub struct ColumnStats {
    pub col: usize,
    pub kind: ColumnKind,
    pub rows: usize,
    pub filled: usize,
    pub distinct: usize,
    /// `true` se parou de contar valores distintos (muitos valores únicos).
    pub distinct_capped: bool,
    /// Valores mais frequentes: (valor, quantidade).
    pub top: Vec<(String, usize)>,
    pub numeric: Option<NumericStats>,
    /// Comprimento em caracteres: (mínimo, máximo, médio).
    pub text_len: Option<(usize, usize, f64)>,
}

pub fn compute(table: &Table, view: &View, col: usize) -> ColumnStats {
    let column = &table.columns[col];
    let n = view.len();

    let mut counts: FxHashMap<&str, usize> = FxHashMap::default();
    let mut capped = false;
    let mut filled = 0usize;
    let (mut len_min, mut len_max, mut len_sum) = (usize::MAX, 0usize, 0usize);
    let mut values: Vec<f64> = Vec::new();

    for i in 0..n {
        let r = view.row(i);
        let s = column.get(r);
        if s.trim().is_empty() {
            continue;
        }
        filled += 1;
        let len = s.chars().count();
        len_min = len_min.min(len);
        len_max = len_max.max(len);
        len_sum += len;
        if let Some(c) = counts.get_mut(s) {
            *c += 1;
        } else if counts.len() < DISTINCT_CAP {
            counts.insert(s, 1);
        } else {
            capped = true;
        }
        if let Some(k) = column.key(r) {
            values.push(k);
        }
    }

    let mut top: Vec<(&str, usize)> = counts.iter().map(|(k, v)| (*k, *v)).collect();
    // Só os mais frequentes; desempate alfabético para ser estável.
    let keep = 10.min(top.len());
    if keep > 0 {
        top.select_nth_unstable_by(keep - 1, |a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        top.truncate(keep);
        top.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    }
    // Se todos os valores são únicos, a lista de "mais comuns" não ajuda.
    let all_unique = counts.len() == filled && filled > 1;
    let top = if all_unique && !column.kind.is_temporal() {
        Vec::new()
    } else {
        top.into_iter().map(|(s, c)| (s.to_owned(), c)).collect()
    };

    let typed = matches!(
        column.kind,
        ColumnKind::Integer | ColumnKind::Decimal | ColumnKind::Date | ColumnKind::DateTime
    );
    let numeric = (typed && !values.is_empty()).then(|| numeric_stats(&mut values));

    ColumnStats {
        col,
        kind: column.kind,
        rows: n,
        filled,
        distinct: counts.len(),
        distinct_capped: capped,
        top,
        numeric,
        text_len: (filled > 0).then(|| (len_min, len_max, len_sum as f64 / filled as f64)),
    }
}

fn numeric_stats(values: &mut [f64]) -> NumericStats {
    let count = values.len();
    let (mut min, mut max, mut sum) = (f64::INFINITY, f64::NEG_INFINITY, 0.0);
    for &v in values.iter() {
        min = min.min(v);
        max = max.max(v);
        sum += v;
    }
    let mean = sum / count as f64;
    let var = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / count as f64;

    let mut histogram = vec![0u32; HISTOGRAM_BINS];
    let span = max - min;
    for &v in values.iter() {
        let idx = if span > 0.0 {
            (((v - min) / span) * HISTOGRAM_BINS as f64) as usize
        } else {
            HISTOGRAM_BINS / 2
        };
        histogram[idx.min(HISTOGRAM_BINS - 1)] += 1;
    }

    let mid = count / 2;
    let cmp = |a: &f64, b: &f64| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal);
    let (_, &mut upper, _) = values.select_nth_unstable_by(mid, cmp);
    let median = if count.is_multiple_of(2) && count > 1 {
        let lower = values[..mid]
            .iter()
            .copied()
            .fold(f64::NEG_INFINITY, f64::max);
        (lower + upper) / 2.0
    } else {
        upper
    };

    NumericStats {
        count,
        min,
        max,
        sum,
        mean,
        median,
        std_dev: var.sqrt(),
        histogram,
    }
}

/// Resumo de uma seleção retangular (para a barra de status, como no Excel).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SelectionSummary {
    pub cells: usize,
    pub filled: usize,
    pub numbers: usize,
    pub sum: f64,
    pub min: f64,
    pub max: f64,
}

impl SelectionSummary {
    pub fn mean(&self) -> Option<f64> {
        (self.numbers > 0).then(|| self.sum / self.numbers as f64)
    }
}

pub fn summarize_selection(
    table: &Table,
    view: &View,
    rows: std::ops::RangeInclusive<usize>,
    cols: &[usize],
) -> SelectionSummary {
    let mut s = SelectionSummary {
        min: f64::INFINITY,
        max: f64::NEG_INFINITY,
        ..Default::default()
    };
    for i in rows {
        let r = view.row(i);
        for &c in cols {
            let column = &table.columns[c];
            s.cells += 1;
            if column.is_empty_at(r) {
                continue;
            }
            s.filled += 1;
            if column.kind.is_numeric()
                && let Some(v) = column.key(r)
            {
                s.numbers += 1;
                s.sum += v;
                s.min = s.min.min(v);
                s.max = s.max.max(v);
            }
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mediana_par_e_impar() {
        let mut v = vec![3.0, 1.0, 2.0];
        assert_eq!(numeric_stats(&mut v).median, 2.0);
        let mut v = vec![4.0, 1.0, 3.0, 2.0];
        assert_eq!(numeric_stats(&mut v).median, 2.5);
    }
}
