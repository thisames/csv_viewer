//! Formatação de números, datas e tamanhos no padrão brasileiro.

use crate::data::infer::civil_from_days;
use crate::data::table::ColumnKind;

/// `1234567` → `"1.234.567"`
pub fn int(n: usize) -> String {
    group(&n.to_string())
}

fn group(digits: &str) -> String {
    let (sign, digits) = match digits.strip_prefix('-') {
        Some(d) => ("-", d),
        None => ("", digits),
    };
    let mut out = String::with_capacity(digits.len() + digits.len() / 3 + 1);
    out.push_str(sign);
    let first = digits.len() % 3;
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (i + 3 - first) % 3 == 0 {
            out.push('.');
        }
        out.push(c);
    }
    out
}

/// Número com casas decimais adaptativas: `1234.5` → `"1.234,5"`.
pub fn num(v: f64) -> String {
    if !v.is_finite() {
        return "—".to_owned();
    }
    if v.fract() == 0.0 && v.abs() < 1e15 {
        return group(&format!("{}", v as i64));
    }
    let abs = v.abs();
    let decimals = if abs >= 1000.0 {
        2
    } else if abs >= 1.0 {
        3
    } else if abs >= 0.01 {
        4
    } else {
        6
    };
    let s = format!("{:.*}", decimals, v);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    let (int_part, frac) = match s.split_once('.') {
        Some((i, f)) => (i, Some(f)),
        None => (s, None),
    };
    let mut out = group(int_part);
    if let Some(f) = frac {
        out.push(',');
        out.push_str(f);
    }
    out
}

/// Número grande abreviado: `2_000_001_000_000.0` → `"2,00 tri"`.
pub fn compact(v: f64) -> String {
    let abs = v.abs();
    let (div, suffix) = if abs >= 1e12 {
        (1e12, " tri")
    } else if abs >= 1e9 {
        (1e9, " bi")
    } else if abs >= 1e6 {
        (1e6, " mi")
    } else {
        return num(v);
    };
    let x = v / div;
    let s = if x.abs() >= 100.0 {
        format!("{x:.0}")
    } else if x.abs() >= 10.0 {
        format!("{x:.1}")
    } else {
        format!("{x:.2}")
    };
    format!("{}{suffix}", s.replace('.', ","))
}

/// Número completo se couber em `max_chars`, senão abreviado.
pub fn num_fit(v: f64, max_chars: usize) -> String {
    let s = num(v);
    if s.chars().count() > max_chars { compact(v) } else { s }
}

/// Formata uma chave numérica de acordo com o tipo da coluna.
pub fn key(v: f64, kind: ColumnKind) -> String {
    match kind {
        ColumnKind::Date => date(v, false),
        ColumnKind::DateTime => date(v, true),
        _ => num(v),
    }
}

/// Dias desde 1970 → `"31/12/2024"` (ou com hora).
pub fn date(days: f64, with_time: bool) -> String {
    let d = days.floor();
    let (y, m, dd) = civil_from_days(d as i64);
    if !with_time {
        return format!("{dd:02}/{m:02}/{y}");
    }
    let secs = ((days - d) * 86_400.0).round() as i64;
    let (h, mi) = (secs / 3600, (secs / 60) % 60);
    format!("{dd:02}/{m:02}/{y} {h:02}:{mi:02}")
}

pub fn percent(part: usize, total: usize) -> String {
    if total == 0 {
        return "0%".to_owned();
    }
    let p = part as f64 * 100.0 / total as f64;
    if p >= 99.95 && part < total {
        "99,9%".to_owned()
    } else if p > 0.0 && p < 0.1 {
        "<0,1%".to_owned()
    } else if p.fract() == 0.0 || p >= 10.0 {
        format!("{}%", p.round() as i64)
    } else {
        format!("{:.1}%", p).replace('.', ",")
    }
}

pub fn bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = n as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 {
        format!("{n} B")
    } else {
        format!("{:.1} {}", v, UNITS[u]).replace('.', ",")
    }
}

pub fn duration(d: std::time::Duration) -> String {
    let ms = d.as_millis();
    if ms < 1000 {
        format!("{ms} ms")
    } else {
        format!("{:.1} s", d.as_secs_f64()).replace('.', ",")
    }
}

/// Nome de coluna estilo planilha: 0 → A, 25 → Z, 26 → AA.
pub fn column_letter(mut i: usize) -> String {
    let mut s = Vec::new();
    loop {
        s.push(b'A' + (i % 26) as u8);
        if i < 26 {
            break;
        }
        i = i / 26 - 1;
    }
    s.reverse();
    String::from_utf8(s).unwrap_or_default()
}

/// "1 linha" / "2 linhas"
pub fn plural(n: usize, one: &str, many: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{} {many}", int(n))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formatos() {
        assert_eq!(int(0), "0");
        assert_eq!(int(999), "999");
        assert_eq!(int(1000), "1.000");
        assert_eq!(int(1234567), "1.234.567");
        assert_eq!(num(1234.5), "1.234,5");
        assert_eq!(num(-1234567.891), "-1.234.567,89");
        assert_eq!(num(0.12345), "0,1235");
        assert_eq!(num(3.0), "3");
        assert_eq!(percent(1, 3), "33%");
        assert_eq!(percent(1, 200), "0,5%");
        assert_eq!(column_letter(0), "A");
        assert_eq!(column_letter(27), "AB");
        assert_eq!(bytes(1536), "1,5 KB");
        assert_eq!(date(0.0, false), "01/01/1970");
        assert_eq!(compact(2_000_001_000_000.0), "2,00 tri");
        assert_eq!(compact(117_416_763.04), "117 mi");
        assert_eq!(num_fit(1234.5, 13), "1.234,5");
    }
}
