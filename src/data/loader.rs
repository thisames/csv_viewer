//! Leitura de arquivos (CSV/TSV/TXT) em segundo plano.

use std::borrow::Cow;
use std::fs::File;
use std::io::{self, BufReader, Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::time::Instant;

use encoding_rs::{Encoding, UTF_8};
use rayon::prelude::*;

use super::infer::{DecimalSep, infer_column};
use super::sniff::{detect_delimiter, detect_encoding};
use super::table::{Column, ColumnBuilder, FileFormat, Profile, SourceInfo, Table};

#[derive(Clone, Debug, PartialEq)]
pub struct LoadOptions {
    pub has_header: bool,
    /// `None` = detectar automaticamente.
    pub delimiter: Option<u8>,
    /// `None` = detectar automaticamente.
    pub encoding: Option<&'static Encoding>,
}

impl Default for LoadOptions {
    fn default() -> Self {
        Self {
            has_header: true,
            delimiter: None,
            encoding: None,
        }
    }
}

/// Estágios de `Progress::stage`: 0 = lendo o arquivo, 1 = analisando colunas.
pub const STAGE_ANALYZING: u8 = 1;

/// Progresso compartilhado com a interface.
#[derive(Default)]
pub struct Progress {
    pub bytes: AtomicU64,
    pub total: AtomicU64,
    pub rows: AtomicU64,
    pub stage: AtomicU8,
    pub cancel: AtomicBool,
}

impl Progress {
    pub fn fraction(&self) -> Option<f32> {
        let total = self.total.load(Ordering::Relaxed);
        (total > 0).then(|| self.bytes.load(Ordering::Relaxed) as f32 / total as f32)
    }
    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

pub struct Loaded {
    pub table: Table,
    pub info: SourceInfo,
}

const CANCELLED: &str = "Leitura cancelada.";

fn extension(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default()
}

/// Lê o arquivo inteiro. Bloqueante: chame numa thread separada.
pub fn load(path: &Path, opts: &LoadOptions, progress: &Progress) -> Result<Loaded, String> {
    let start = Instant::now();
    let meta = std::fs::metadata(path).map_err(|e| io_error(path, &e))?;
    if meta.is_dir() {
        return Err("Isso é uma pasta, não um arquivo.".to_owned());
    }
    progress.total.store(meta.len(), Ordering::Relaxed);

    let mut loaded = load_delimited(path, opts, progress, meta.len())?;
    if progress.cancelled() {
        return Err(CANCELLED.to_owned());
    }

    progress.stage.store(STAGE_ANALYZING, Ordering::Relaxed);
    let decimal_fallback = match loaded.info.delimiter {
        Some(b';') => DecimalSep::Comma,
        _ => DecimalSep::Dot,
    };
    analyze_columns(&mut loaded.table, decimal_fallback);

    loaded.info.file_size = meta.len();
    loaded.info.load_time = start.elapsed();
    Ok(loaded)
}

fn io_error(path: &Path, e: &io::Error) -> String {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    match e.kind() {
        io::ErrorKind::NotFound => format!("Arquivo não encontrado: {name}"),
        io::ErrorKind::PermissionDenied => format!("Sem permissão para ler {name}"),
        _ => format!("Não foi possível ler {name}: {e}"),
    }
}

/// Leitor que conta bytes lidos para a barra de progresso.
struct CountingReader<'a, R> {
    inner: R,
    progress: &'a Progress,
    base: u64,
    read: u64,
}

impl<R: Read> Read for CountingReader<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.progress.cancelled() {
            return Err(io::Error::new(io::ErrorKind::Interrupted, CANCELLED));
        }
        let n = self.inner.read(buf)?;
        self.read += n as u64;
        self.progress
            .bytes
            .store(self.base + self.read, Ordering::Relaxed);
        Ok(n)
    }
}

// ---------------------------------------------------------------------------
// CSV / TSV

fn load_delimited(
    path: &Path,
    opts: &LoadOptions,
    progress: &Progress,
    file_size: u64,
) -> Result<Loaded, String> {
    let mut file = File::open(path).map_err(|e| io_error(path, &e))?;

    // Amostra do começo para detectar codificação e separador.
    let mut sample = Vec::with_capacity(256 * 1024);
    (&mut file)
        .take(256 * 1024)
        .read_to_end(&mut sample)
        .map_err(|e| io_error(path, &e))?;
    let complete = (sample.len() as u64) >= file_size;

    let detected = detect_encoding(&sample, complete);
    let (encoding, bom_len) = match opts.encoding {
        Some(forced) => {
            // Ainda pula o BOM se ele combinar com a codificação escolhida.
            let bom = if forced == detected.encoding {
                detected.bom_len
            } else {
                0
            };
            (forced, bom)
        }
        None => (detected.encoding, detected.bom_len),
    };

    let utf16 = encoding == encoding_rs::UTF_16LE || encoding == encoding_rs::UTF_16BE;

    let sample_text = {
        let body = &sample[bom_len.min(sample.len())..];
        let (text, _) = encoding.decode_without_bom_handling(body);
        text.into_owned()
    };
    let delimiter = opts
        .delimiter
        .unwrap_or_else(|| detect_delimiter(&sample_text, &extension(path)));
    drop(sample_text);

    let mut sink = Sink {
        builders: Vec::new(),
        header: None,
        has_header: opts.has_header,
        rows: 0,
        ragged: 0,
        file_size,
    };

    let mut record = csv::ByteRecord::new();
    if utf16 {
        // UTF-16: converte o arquivo inteiro para UTF-8 antes.
        let mut raw = Vec::with_capacity(file_size as usize);
        file.seek(SeekFrom::Start(0)).map_err(|e| io_error(path, &e))?;
        CountingReader {
            inner: &mut file,
            progress,
            base: 0,
            read: 0,
        }
        .read_to_end(&mut raw)
        .map_err(|e| read_error(path, &e))?;
        let (text, _) = encoding.decode_without_bom_handling(&raw[bom_len.min(raw.len())..]);
        let text = text.into_owned();
        drop(raw);
        let mut rdr = csv_reader(delimiter, text.as_bytes());
        loop {
            match rdr.read_byte_record(&mut record) {
                Ok(true) => sink.push(&record, FieldDecoder::Utf8, progress)?,
                Ok(false) => break,
                Err(e) => return Err(csv_error(&e)),
            }
        }
    } else {
        file.seek(SeekFrom::Start(bom_len as u64))
            .map_err(|e| io_error(path, &e))?;
        let reader = CountingReader {
            inner: BufReader::with_capacity(1 << 20, file),
            progress,
            base: bom_len as u64,
            read: 0,
        };
        let mut rdr = csv_reader(delimiter, reader);
        let decoder = if encoding == UTF_8 {
            FieldDecoder::Utf8
        } else {
            FieldDecoder::Other(encoding)
        };
        loop {
            match rdr.read_byte_record(&mut record) {
                Ok(true) => sink.push(&record, decoder, progress)?,
                Ok(false) => break,
                Err(e) => {
                    if progress.cancelled() {
                        return Err(CANCELLED.to_owned());
                    }
                    return Err(csv_error(&e));
                }
            }
        }
    }

    let rows = sink.rows;
    progress.rows.store(rows as u64, Ordering::Relaxed);
    progress.bytes.store(file_size, Ordering::Relaxed);

    let header_names = sink.header.unwrap_or_default();
    let columns = finish_columns(sink.builders, rows, &header_names);

    Ok(Loaded {
        table: Table { columns, rows },
        info: SourceInfo {
            path: path.to_path_buf(),
            format: FileFormat::Delimited,
            encoding: Some(encoding_label(encoding)),
            delimiter: Some(delimiter),
            sheets: Vec::new(),
            sheet: None,
            file_size,
            load_time: Default::default(),
            ragged_rows: sink.ragged,
        },
    })
}

/// Como transformar os bytes de cada campo em texto.
#[derive(Clone, Copy)]
enum FieldDecoder {
    Utf8,
    Other(&'static Encoding),
}

impl FieldDecoder {
    #[inline]
    fn decode<'a>(self, field: &'a [u8]) -> Cow<'a, str> {
        match self {
            Self::Utf8 => String::from_utf8_lossy(field),
            Self::Other(enc) => enc.decode_without_bom_handling(field).0,
        }
    }
}

/// Recebe os registros do CSV e monta as colunas.
struct Sink {
    builders: Vec<ColumnBuilder>,
    header: Option<Vec<String>>,
    has_header: bool,
    rows: usize,
    ragged: usize,
    file_size: u64,
}

impl Sink {
    fn push(
        &mut self,
        record: &csv::ByteRecord,
        decoder: FieldDecoder,
        progress: &Progress,
    ) -> Result<(), String> {
        if self.has_header && self.header.is_none() {
            let names: Vec<String> = record
                .iter()
                .map(|f| decoder.decode(f).trim().to_owned())
                .collect();
            self.builders = names.iter().map(|n| ColumnBuilder::new(n.clone())).collect();
            // Reserva aproximada para evitar realocações em arquivos grandes.
            let ncols = self.builders.len().max(1);
            let per_col = (self.file_size as usize / ncols).min(1 << 30);
            let est_rows = (self.file_size as usize / (ncols * 6).max(16)).min(20_000_000);
            for b in &mut self.builders {
                b.reserve(est_rows, per_col);
            }
            self.header = Some(names);
            return Ok(());
        }
        let expected = self.header.as_ref().map_or(self.builders.len(), Vec::len);
        if record.len() != expected && (self.header.is_some() || self.rows > 0) {
            self.ragged += 1;
        }
        while self.builders.len() < record.len() {
            let mut b = ColumnBuilder::new(String::new());
            b.pad_to(self.rows);
            self.builders.push(b);
        }
        for (i, field) in record.iter().enumerate() {
            self.builders[i]
                .push(&decoder.decode(field))
                .map_err(|_| "Arquivo grande demais (uma coluna passou de 4 GB).".to_owned())?;
        }
        for b in self.builders.iter_mut().skip(record.len()) {
            b.push("")
                .map_err(|_| "Arquivo grande demais.".to_owned())?;
        }
        self.rows += 1;
        if self.rows.is_multiple_of(8192) {
            progress.rows.store(self.rows as u64, Ordering::Relaxed);
            if progress.cancelled() {
                return Err(CANCELLED.to_owned());
            }
        }
        Ok(())
    }
}

fn csv_reader<R: Read>(delimiter: u8, r: R) -> csv::Reader<R> {
    csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .has_headers(false)
        .flexible(true)
        .buffer_capacity(1 << 20)
        .from_reader(r)
}

fn csv_error(e: &csv::Error) -> String {
    match e.kind() {
        csv::ErrorKind::Io(io) if io.kind() == io::ErrorKind::Interrupted => CANCELLED.to_owned(),
        _ => match e.position() {
            Some(pos) => format!("Erro ao ler o CSV na linha {}: {e}", pos.line()),
            None => format!("Erro ao ler o CSV: {e}"),
        },
    }
}

fn read_error(path: &Path, e: &io::Error) -> String {
    if e.kind() == io::ErrorKind::Interrupted {
        CANCELLED.to_owned()
    } else {
        io_error(path, e)
    }
}

pub fn encoding_label(enc: &'static Encoding) -> &'static str {
    match enc.name() {
        "UTF-8" => "UTF-8",
        "windows-1252" => "Windows-1252 (Latin-1)",
        "ISO-8859-15" => "ISO-8859-15",
        "ISO-8859-2" => "ISO-8859-2",
        "UTF-16LE" => "UTF-16 LE",
        "UTF-16BE" => "UTF-16 BE",
        "IBM866" => "IBM866",
        other => other,
    }
}

/// Dá nomes às colunas sem cabeçalho e completa linhas curtas.
fn finish_columns(builders: Vec<ColumnBuilder>, rows: usize, header: &[String]) -> Vec<Column> {
    builders
        .into_iter()
        .enumerate()
        .map(|(i, mut b)| {
            b.pad_to(rows);
            let mut col = b.finish();
            let name = header.get(i).map(|s| s.trim()).unwrap_or("");
            col.name = if name.is_empty() {
                format!("Coluna {}", i + 1)
            } else {
                name.trim_start_matches('\u{feff}').to_owned()
            };
            col
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Análise de tipos

fn analyze_columns(table: &mut Table, decimal_fallback: DecimalSep) {
    let rows = table.rows;
    table.columns.par_iter_mut().for_each(|col| {
        let inferred = {
            let c: &Column = col;
            infer_column(rows, |r| c.get(r), decimal_fallback)
        };
        col.kind = inferred.kind;
        col.keys = inferred.keys;
        col.profile = build_profile(col, rows);
        col.typical_chars = typical_chars(col, rows);
    });
}

fn build_profile(col: &Column, rows: usize) -> Profile {
    let filled = (0..rows).filter(|&r| !col.is_empty_at(r)).count();
    let sparkline = match (&col.keys, col.kind) {
        (Some(keys), k) if k.is_numeric() || k.is_temporal() => {
            let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
            for &v in keys.iter().filter(|v| !v.is_nan()) {
                lo = lo.min(v);
                hi = hi.max(v);
            }
            if lo.is_finite() && hi.is_finite() {
                const BINS: usize = 18;
                let mut bins = vec![0u32; BINS];
                let span = hi - lo;
                for &v in keys.iter().filter(|v| !v.is_nan()) {
                    let idx = if span > 0.0 {
                        (((v - lo) / span) * BINS as f64) as usize
                    } else {
                        BINS / 2
                    };
                    bins[idx.min(BINS - 1)] += 1;
                }
                Some(bins)
            } else {
                None
            }
        }
        _ => None,
    };
    Profile { filled, sparkline }
}

/// Comprimento "típico" (percentil ~90 da amostra) para a largura inicial.
fn typical_chars(col: &Column, rows: usize) -> usize {
    let stride = (rows / 400).max(1);
    let mut lens: Vec<usize> = (0..rows)
        .step_by(stride)
        .take(400)
        .map(|r| {
            let s = col.get(r);
            let first_line = s.split(['\n', '\r']).next().unwrap_or("");
            first_line.chars().count().min(80)
        })
        .collect();
    if lens.is_empty() {
        return 0;
    }
    lens.sort_unstable();
    lens[(lens.len() * 9 / 10).min(lens.len() - 1)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::table::ColumnKind;

    fn write_tmp(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("csv_viwer_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join(name);
        std::fs::write(&p, bytes).unwrap();
        p
    }

    #[test]
    fn le_csv_brasileiro_em_latin1() {
        // "nome;valor\nJoão;1.234,50\nAna;10,00\n" em Windows-1252
        let mut bytes = b"nome;valor\nJo".to_vec();
        bytes.push(0xE3);
        bytes.extend_from_slice(b"o;1.234,50\nAna;10,00\n");
        let p = write_tmp("latin1.csv", &bytes);
        let loaded = load(&p, &LoadOptions::default(), &Progress::default()).unwrap();
        assert_eq!(loaded.info.delimiter, Some(b';'));
        assert_eq!(loaded.table.rows, 2);
        assert_eq!(loaded.table.columns[0].name, "nome");
        assert_eq!(loaded.table.cell(0, 0), "João");
        assert_eq!(loaded.table.columns[1].kind, ColumnKind::Decimal);
        assert_eq!(loaded.table.columns[1].key(0), Some(1234.5));
    }

    #[test]
    fn le_csv_com_bom_aspas_e_linhas_irregulares() {
        let p = write_tmp(
            "bom.csv",
            "\u{feff}id,texto\n1,\"olá, mundo\"\n2,\"linha\nquebrada\"\n3\n4,a,extra\n".as_bytes(),
        );
        let loaded = load(&p, &LoadOptions::default(), &Progress::default()).unwrap();
        let t = &loaded.table;
        assert_eq!(t.columns[0].name, "id");
        assert_eq!(t.rows, 4);
        assert_eq!(t.cell(0, 1), "olá, mundo");
        assert_eq!(t.cell(1, 1), "linha\nquebrada");
        assert_eq!(t.cell(2, 1), "");
        assert_eq!(t.columns.len(), 3);
        assert_eq!(t.columns[2].name, "Coluna 3");
        assert_eq!(t.cell(3, 2), "extra");
        assert_eq!(t.columns[0].kind, ColumnKind::Integer);
    }

    #[test]
    fn le_utf16_com_bom() {
        let text = "cidade\tvalor\nSão Paulo\t1,5\nBelém\t2\n";
        let mut bytes = vec![0xFF, 0xFE];
        for u in text.encode_utf16() {
            bytes.extend_from_slice(&u.to_le_bytes());
        }
        let p = write_tmp("utf16.txt", &bytes);
        let loaded = load(&p, &LoadOptions::default(), &Progress::default()).unwrap();
        assert_eq!(loaded.info.delimiter, Some(b'\t'));
        assert_eq!(loaded.table.columns[0].name, "cidade");
        assert_eq!(loaded.table.cell(0, 0), "São Paulo");
        assert_eq!(loaded.table.cell(1, 0), "Belém");
        assert_eq!(loaded.table.rows, 2);
    }
}
