//! Detecção automática de codificação (UTF-8, Windows-1252…) e de separador.

use encoding_rs::{Encoding, UTF_8, UTF_16BE, UTF_16LE};

/// Codificação detectada e quantos bytes de BOM pular.
pub struct DetectedEncoding {
    pub encoding: &'static Encoding,
    pub bom_len: usize,
}

/// Descobre a codificação a partir do começo do arquivo.
///
/// `is_complete` indica se `sample` é o arquivo inteiro (importa para saber se
/// uma sequência UTF-8 cortada no fim da amostra é erro ou só truncamento).
pub fn detect_encoding(sample: &[u8], is_complete: bool) -> DetectedEncoding {
    if sample.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return DetectedEncoding {
            encoding: UTF_8,
            bom_len: 3,
        };
    }
    if sample.starts_with(&[0xFF, 0xFE]) {
        return DetectedEncoding {
            encoding: UTF_16LE,
            bom_len: 2,
        };
    }
    if sample.starts_with(&[0xFE, 0xFF]) {
        return DetectedEncoding {
            encoding: UTF_16BE,
            bom_len: 2,
        };
    }
    // UTF-16 sem BOM: muitos zeros alternados.
    if sample.len() >= 64 {
        let n = sample.len().min(4096) & !1;
        let zeros_even = sample[..n].iter().step_by(2).filter(|&&b| b == 0).count();
        let zeros_odd = sample[1..n].iter().step_by(2).filter(|&&b| b == 0).count();
        let half = n / 2;
        if zeros_odd > half * 4 / 10 && zeros_even < half / 20 {
            return DetectedEncoding {
                encoding: UTF_16LE,
                bom_len: 0,
            };
        }
        if zeros_even > half * 4 / 10 && zeros_odd < half / 20 {
            return DetectedEncoding {
                encoding: UTF_16BE,
                bom_len: 0,
            };
        }
    }

    match std::str::from_utf8(sample) {
        Ok(_) => DetectedEncoding {
            encoding: UTF_8,
            bom_len: 0,
        },
        // Sequência cortada no fim da amostra: ainda é UTF-8.
        Err(e) if e.error_len().is_none() && !is_complete => DetectedEncoding {
            encoding: UTF_8,
            bom_len: 0,
        },
        Err(_) => {
            let mut det = chardetng::EncodingDetector::new(chardetng::Iso2022JpDetection::Deny);
            det.feed(sample, is_complete);
            // A dica ".br" puxa para as codificações usadas em português.
            let enc = det.guess(Some(b"br"), chardetng::Utf8Detection::Allow);
            DetectedEncoding {
                encoding: enc,
                bom_len: 0,
            }
        }
    }
}

/// Escolhe o separador que produz o número de campos mais consistente.
pub fn detect_delimiter(sample: &str, extension: &str) -> u8 {
    if matches!(extension, "tsv" | "tab") {
        return b'\t';
    }
    if extension == "psv" {
        return b'|';
    }
    // Usa só linhas completas.
    let text = match sample.rfind('\n') {
        Some(i) if i > 0 => &sample[..i],
        _ => sample,
    };

    let mut best = (0.0f64, b',');
    for &d in b",;\t|" {
        let mut rdr = csv::ReaderBuilder::new()
            .delimiter(d)
            .has_headers(false)
            .flexible(true)
            .from_reader(text.as_bytes());
        let mut counts: Vec<usize> = Vec::new();
        let mut rec = csv::ByteRecord::new();
        while counts.len() < 300 {
            match rdr.read_byte_record(&mut rec) {
                Ok(true) => counts.push(rec.len()),
                _ => break,
            }
        }
        if counts.is_empty() {
            continue;
        }
        // Moda do número de campos.
        let mut freq = std::collections::HashMap::<usize, usize>::new();
        for &c in &counts {
            *freq.entry(c).or_default() += 1;
        }
        let (&mode, &mode_freq) = freq
            .iter()
            .max_by_key(|&(&k, &v)| (v, k))
            .expect("counts não está vazio");
        if mode <= 1 {
            continue;
        }
        let consistency = mode_freq as f64 / counts.len() as f64;
        // Desempate leve a favor de mais colunas.
        let score = consistency * 1000.0 + (mode.min(200) as f64) * 0.5;
        if score > best.0 {
            best = (score, d);
        }
    }
    if best.0 == 0.0 && text.contains('\t') {
        return b'\t';
    }
    best.1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn separadores() {
        assert_eq!(detect_delimiter("a;b;c\n1;2,5;3\n4;5;6\n", "csv"), b';');
        assert_eq!(detect_delimiter("a,b,c\n1,2,3\n4,5,6\n", "csv"), b',');
        assert_eq!(detect_delimiter("a\tb\n1\t2\n", "txt"), b'\t');
        assert_eq!(
            detect_delimiter("nome,obs\n\"Silva; João\",\"x, y\"\nAna,ok\n", "csv"),
            b','
        );
    }

    #[test]
    fn codificacao() {
        assert_eq!(detect_encoding("ação".as_bytes(), true).encoding, UTF_8);
        let latin1 = [b'a', 0xE7, 0xE3, b'o', b';', b'x', b'\n'];
        let enc = detect_encoding(&latin1, true).encoding;
        assert_ne!(enc, UTF_8);
        let (s, _, _) = enc.decode(&latin1);
        assert!(s.starts_with("ação"));
    }
}
