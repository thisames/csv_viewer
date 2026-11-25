//! Armazenamento da tabela em memória.
//!
//! Cada coluna guarda o texto de todas as células concatenado num único
//! `String`, mais um vetor de offsets. É bem mais compacto (e rápido de
//! percorrer) do que um `String` por célula.

use std::path::PathBuf;
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ColumnKind {
    Text,
    Integer,
    Decimal,
    Date,
    DateTime,
    Bool,
    Empty,
}

impl ColumnKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Text => "Texto",
            Self::Integer => "Número inteiro",
            Self::Decimal => "Número decimal",
            Self::Date => "Data",
            Self::DateTime => "Data e hora",
            Self::Bool => "Sim / Não",
            Self::Empty => "Vazia",
        }
    }

    pub fn is_numeric(self) -> bool {
        matches!(self, Self::Integer | Self::Decimal)
    }

    pub fn is_temporal(self) -> bool {
        matches!(self, Self::Date | Self::DateTime)
    }

    /// Números alinham à direita, como numa planilha.
    pub fn align_right(self) -> bool {
        self.is_numeric()
    }
}

/// Resumo leve calculado no carregamento (usado na lista de colunas).
#[derive(Clone, Debug, Default)]
pub struct Profile {
    pub filled: usize,
    /// Histograma pequeno (só para colunas numéricas e de data).
    pub sparkline: Option<Vec<u32>>,
}

pub struct Column {
    pub name: String,
    pub kind: ColumnKind,
    data: String,
    offsets: Vec<u32>,
    /// Chave numérica por linha para ordenar e calcular estatísticas
    /// (NaN = vazio ou inválido). Só existe para colunas tipadas.
    pub keys: Option<Vec<f64>>,
    pub profile: Profile,
    /// Largura (em caracteres) aproximada do conteúdo, para a largura inicial.
    pub typical_chars: usize,
}

impl Column {
    #[inline]
    pub fn get(&self, row: usize) -> &str {
        let s = self.offsets[row] as usize;
        let e = self.offsets[row + 1] as usize;
        &self.data[s..e]
    }

    #[inline]
    pub fn is_empty_at(&self, row: usize) -> bool {
        self.offsets[row] == self.offsets[row + 1]
    }

    #[inline]
    pub fn key(&self, row: usize) -> Option<f64> {
        self.keys.as_ref().map(|k| k[row]).filter(|v| !v.is_nan())
    }

    pub fn memory_bytes(&self) -> usize {
        self.data.capacity()
            + self.offsets.capacity() * 4
            + self.keys.as_ref().map_or(0, |k| k.capacity() * 8)
    }
}

/// Erro quando uma coluna passa de 4 GB de texto.
#[derive(Debug)]
pub struct ColumnTooLarge;

pub struct ColumnBuilder {
    pub name: String,
    data: String,
    offsets: Vec<u32>,
}

impl ColumnBuilder {
    pub fn new(name: String) -> Self {
        Self {
            name,
            data: String::new(),
            offsets: vec![0],
        }
    }

    pub fn reserve(&mut self, rows: usize, bytes: usize) {
        self.offsets.reserve(rows);
        self.data.reserve(bytes);
    }

    #[inline]
    pub fn push(&mut self, value: &str) -> Result<(), ColumnTooLarge> {
        self.data.push_str(value);
        let end = u32::try_from(self.data.len()).map_err(|_| ColumnTooLarge)?;
        self.offsets.push(end);
        Ok(())
    }

    /// Completa com células vazias até `rows` linhas.
    pub fn pad_to(&mut self, rows: usize) {
        let end = self.data.len() as u32;
        while self.offsets.len() - 1 < rows {
            self.offsets.push(end);
        }
    }

    pub fn finish(mut self) -> Column {
        self.data.shrink_to_fit();
        self.offsets.shrink_to_fit();
        Column {
            name: self.name,
            kind: ColumnKind::Text,
            data: self.data,
            offsets: self.offsets,
            keys: None,
            profile: Profile::default(),
            typical_chars: 0,
        }
    }
}

pub struct Table {
    pub columns: Vec<Column>,
    pub rows: usize,
}

impl Table {
    #[inline]
    pub fn cell(&self, row: usize, col: usize) -> &str {
        self.columns[col].get(row)
    }

    pub fn memory_bytes(&self) -> usize {
        self.columns.iter().map(Column::memory_bytes).sum()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileFormat {
    Delimited,
    Workbook,
}

/// De onde a tabela veio e como foi lida.
#[derive(Clone, Debug)]
pub struct SourceInfo {
    pub path: PathBuf,
    pub format: FileFormat,
    pub encoding: Option<&'static str>,
    pub delimiter: Option<u8>,
    pub sheets: Vec<String>,
    pub sheet: Option<String>,
    pub file_size: u64,
    pub load_time: Duration,
    /// Linhas com número de campos diferente do cabeçalho.
    pub ragged_rows: usize,
}

impl SourceInfo {
    pub fn format_label(&self) -> &'static str {
        let ext = self
            .path
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .unwrap_or_default();
        match (self.format, ext.as_str()) {
            (FileFormat::Workbook, "ods") => "OpenDocument",
            (FileFormat::Workbook, _) => "Excel",
            (FileFormat::Delimited, "tsv" | "tab") => "TSV",
            (FileFormat::Delimited, "txt") => "Texto",
            (FileFormat::Delimited, _) => "CSV",
        }
    }
}

pub fn delimiter_label(d: u8) -> &'static str {
    match d {
        b',' => "vírgula",
        b';' => "ponto e vírgula",
        b'\t' => "tab",
        b'|' => "barra vertical",
        b':' => "dois-pontos",
        b' ' => "espaço",
        _ => "outro",
    }
}

pub fn delimiter_symbol(d: u8) -> String {
    match d {
        b'\t' => "⇥".to_owned(),
        b' ' => "␣".to_owned(),
        other => (other as char).to_string(),
    }
}
