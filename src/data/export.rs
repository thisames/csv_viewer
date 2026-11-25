//! Exportação da visão atual (filtros, ordem e colunas visíveis) para CSV.

use std::io::Write;
use std::path::Path;

use super::query::View;
use super::table::Table;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportStyle {
    /// Vírgula, UTF-8 sem BOM (padrão internacional).
    Standard,
    /// Ponto e vírgula, UTF-8 com BOM: abre certinho no Excel em português.
    ExcelBr,
}

pub fn export_csv(
    table: &Table,
    view: &View,
    cols: &[usize],
    path: &Path,
    style: ExportStyle,
) -> Result<usize, String> {
    let file = std::fs::File::create(path).map_err(|e| format!("Não foi possível criar o arquivo: {e}"))?;
    let mut out = std::io::BufWriter::with_capacity(1 << 20, file);
    let delimiter = match style {
        ExportStyle::Standard => b',',
        ExportStyle::ExcelBr => {
            out.write_all(&[0xEF, 0xBB, 0xBF]).map_err(|e| e.to_string())?;
            b';'
        }
    };
    let mut w = csv::WriterBuilder::new()
        .delimiter(delimiter)
        .from_writer(out);
    w.write_record(cols.iter().map(|&c| table.columns[c].name.as_str()))
        .map_err(|e| e.to_string())?;
    for i in 0..view.len() {
        let r = view.row(i);
        w.write_record(cols.iter().map(|&c| table.cell(r, c)))
            .map_err(|e| e.to_string())?;
    }
    w.flush().map_err(|e| e.to_string())?;
    Ok(view.len())
}

/// Texto TSV (para colar no Excel/Sheets) de um retângulo da visão.
pub fn selection_tsv(
    table: &Table,
    view: &View,
    rows: std::ops::RangeInclusive<usize>,
    cols: &[usize],
    with_header: bool,
) -> String {
    let mut s = String::new();
    let clean = |v: &str, s: &mut String| {
        if v.contains(['\t', '\n', '\r', '"']) {
            s.push('"');
            s.push_str(&v.replace('"', "\"\""));
            s.push('"');
        } else {
            s.push_str(v);
        }
    };
    if with_header {
        for (j, &c) in cols.iter().enumerate() {
            if j > 0 {
                s.push('\t');
            }
            clean(&table.columns[c].name, &mut s);
        }
        s.push('\n');
    }
    let single = rows.start() == rows.end() && cols.len() == 1 && !with_header;
    for i in rows {
        let r = view.row(i);
        for (j, &c) in cols.iter().enumerate() {
            if j > 0 {
                s.push('\t');
            }
            if single {
                // Uma célula só: copia o texto exatamente como está.
                s.push_str(table.cell(r, c));
            } else {
                clean(table.cell(r, c), &mut s);
            }
        }
        if !single {
            s.push('\n');
        }
    }
    if s.ends_with('\n') {
        s.pop();
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::table::ColumnBuilder;

    fn sample() -> Table {
        let mut a = ColumnBuilder::new("nome".into());
        let mut b = ColumnBuilder::new("obs".into());
        for (x, y) in [("Ana", "linha 1\nlinha 2"), ("João", "tem\ttab"), ("Zé", "ok")] {
            a.push(x).unwrap();
            b.push(y).unwrap();
        }
        Table {
            columns: vec![a.finish(), b.finish()],
            rows: 3,
        }
    }

    #[test]
    fn copia_celula_unica_sem_aspas() {
        let t = sample();
        let v = View::All(3);
        assert_eq!(selection_tsv(&t, &v, 0..=0, &[1], false), "linha 1\nlinha 2");
    }

    #[test]
    fn copia_intervalo_como_tsv() {
        let t = sample();
        let v = View::Rows(vec![2, 0]);
        let s = selection_tsv(&t, &v, 0..=1, &[0, 1], true);
        assert_eq!(s, "nome\tobs\nZé\tok\nAna\t\"linha 1\nlinha 2\"");
    }

    #[test]
    fn exporta_para_excel_br() {
        let t = sample();
        let dir = std::env::temp_dir().join(format!("csv_viwer_exp_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("saida.csv");
        let n = export_csv(&t, &View::Rows(vec![1]), &[0, 1], &p, ExportStyle::ExcelBr).unwrap();
        assert_eq!(n, 1);
        let bytes = std::fs::read(&p).unwrap();
        assert!(bytes.starts_with(&[0xEF, 0xBB, 0xBF]));
        let text = String::from_utf8(bytes[3..].to_vec()).unwrap();
        assert_eq!(text, "nome;obs\nJoão;tem\ttab\n");
    }
}
