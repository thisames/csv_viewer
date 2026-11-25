//! Reconhecimento de tipos: números (formato brasileiro e americano), datas e
//! valores booleanos.

use super::table::ColumnKind;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecimalSep {
    /// `1,234.56`
    Dot,
    /// `1.234,56`
    Comma,
}

/// Ordem de dia/mês em datas com barra.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DateOrder {
    /// `31/12/2024` (padrão no Brasil)
    Dmy,
    /// `12/31/2024`
    Mdy,
}

/// Valores que significam "sem dado" e não contam contra o tipo da coluna.
pub fn is_null_token(s: &str) -> bool {
    let t = s.trim();
    if t.is_empty() {
        return true;
    }
    if t.len() > 6 {
        return false;
    }
    matches!(
        t.to_ascii_lowercase().as_str(),
        "-" | "--" | "na" | "n/a" | "n.a." | "nan" | "null" | "none" | "nil" | "#n/d" | "#n/a"
            | "n/d" | "?" | "s/d"
    )
}

fn trim_ws(s: &str) -> &str {
    s.trim_matches(|c: char| c.is_whitespace() || c == '\u{a0}' || c == '\u{202f}')
}

/// Interpreta um número. Aceita sinal, parênteses contábeis, moeda (R$, $, €…),
/// sufixo `%`, separador de milhar compatível com o separador decimal e
/// notação científica.
pub fn parse_number(s: &str, sep: DecimalSep) -> Option<f64> {
    let mut t = trim_ws(s);
    if t.is_empty() || t.len() > 64 {
        return None;
    }

    let mut negative = false;
    if t.starts_with('(') && t.ends_with(')') && t.len() > 2 {
        negative = true;
        t = trim_ws(&t[1..t.len() - 1]);
    }
    let take_sign = |t: &mut &str, negative: &mut bool| {
        if let Some(rest) = t.strip_prefix('-') {
            *negative = !*negative;
            *t = trim_ws(rest);
        } else if let Some(rest) = t.strip_prefix('+') {
            *t = trim_ws(rest);
        } else if let Some(rest) = t.strip_prefix('\u{2212}') {
            *negative = !*negative;
            *t = trim_ws(rest);
        }
    };
    take_sign(&mut t, &mut negative);
    for prefix in ["R$", "US$", "U$", "$", "€", "£", "¥"] {
        if let Some(rest) = t.strip_prefix(prefix) {
            t = trim_ws(rest);
            take_sign(&mut t, &mut negative);
            break;
        }
    }
    if let Some(rest) = t.strip_suffix('%') {
        t = trim_ws(rest);
    }
    if t.is_empty() {
        return None;
    }

    let (thousands, decimal) = match sep {
        DecimalSep::Dot => (b',', b'.'),
        DecimalSep::Comma => (b'.', b','),
    };

    let bytes = t.as_bytes();
    // Buffer na pilha: evita uma alocação por célula (t tem no máximo 64 bytes).
    let mut clean = NumBuf::default();
    if negative {
        clean.push(b'-');
    }

    let mut i = 0;
    let mut int_digits = 0usize;
    let mut group_len = 0usize;
    let mut groups = 0usize;
    // Parte inteira (com separadores de milhar opcionais).
    while i < bytes.len() {
        let b = bytes[i];
        if b.is_ascii_digit() {
            clean.push(b);
            int_digits += 1;
            group_len += 1;
            i += 1;
        } else if b == thousands || b == b' ' {
            // Separador de milhar: grupos seguintes precisam ter 3 dígitos.
            if int_digits == 0 {
                return None;
            }
            if (groups == 0 && group_len > 3) || (groups > 0 && group_len != 3) {
                return None;
            }
            groups += 1;
            group_len = 0;
            i += 1;
        } else {
            break;
        }
    }
    if groups > 0 && group_len != 3 {
        return None;
    }

    let mut frac_digits = 0usize;
    if i < bytes.len() && bytes[i] == decimal {
        clean.push(b'.');
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            clean.push(bytes[i]);
            frac_digits += 1;
            i += 1;
        }
        if frac_digits == 0 {
            return None;
        }
    }
    if int_digits == 0 && frac_digits == 0 {
        return None;
    }

    // Notação científica (só sem separador de milhar).
    if i < bytes.len() && (bytes[i] == b'e' || bytes[i] == b'E') && groups == 0 {
        let mut j = i + 1;
        clean.push(b'e');
        if j < bytes.len() && (bytes[j] == b'+' || bytes[j] == b'-') {
            clean.push(bytes[j]);
            j += 1;
        }
        let start = j;
        while j < bytes.len() && bytes[j].is_ascii_digit() {
            clean.push(bytes[j]);
            j += 1;
        }
        if j == start {
            return None;
        }
        i = j;
    }

    if i != bytes.len() {
        return None;
    }
    clean.as_str()?.parse::<f64>().ok().filter(|v| v.is_finite())
}

/// Pequeno buffer de bytes ASCII na pilha.
struct NumBuf {
    buf: [u8; 80],
    len: usize,
}

impl Default for NumBuf {
    fn default() -> Self {
        Self {
            buf: [0; 80],
            len: 0,
        }
    }
}

impl NumBuf {
    #[inline]
    fn push(&mut self, b: u8) {
        if self.len < self.buf.len() {
            self.buf[self.len] = b;
            self.len += 1;
        }
    }
    fn as_str(&self) -> Option<&str> {
        std::str::from_utf8(&self.buf[..self.len]).ok()
    }
}

/// Decide o separador decimal mais provável de uma coluna olhando amostras.
pub fn detect_decimal_sep<'a>(
    samples: impl Iterator<Item = &'a str>,
    fallback: DecimalSep,
) -> DecimalSep {
    let mut comma_votes = 0i64;
    let mut dot_votes = 0i64;
    for s in samples {
        let t = trim_ws(s);
        let commas = t.matches(',').count();
        let dots = t.matches('.').count();
        if commas > 0 && dots > 0 {
            if t.rfind(',') > t.rfind('.') {
                comma_votes += 3;
            } else {
                dot_votes += 3;
            }
        } else if commas > 1 {
            dot_votes += 2; // 1,234,567
        } else if dots > 1 {
            comma_votes += 2; // 1.234.567
        } else if commas == 1 {
            let after = t.rsplit(',').next().unwrap_or("");
            let digits = after.chars().take_while(char::is_ascii_digit).count();
            if digits != 3 {
                comma_votes += 2; // 12,5
            }
        } else if dots == 1 {
            let after = t.rsplit('.').next().unwrap_or("");
            let digits = after.chars().take_while(char::is_ascii_digit).count();
            if digits != 3 {
                dot_votes += 2; // 12.5
            }
        }
    }
    match comma_votes.cmp(&dot_votes) {
        std::cmp::Ordering::Greater => DecimalSep::Comma,
        std::cmp::Ordering::Less => DecimalSep::Dot,
        std::cmp::Ordering::Equal => fallback,
    }
}

pub fn parse_bool(s: &str) -> Option<bool> {
    let t = trim_ws(s);
    if t.len() > 10 {
        return None;
    }
    match t.to_lowercase().as_str() {
        "true" | "verdadeiro" | "sim" | "yes" => Some(true),
        "false" | "falso" | "não" | "nao" | "no" => Some(false),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Datas

/// Dias desde 1970-01-01 (algoritmo de Howard Hinnant).
pub fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let m = m as i64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Inverso de [`days_from_civil`].
pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn days_in_month(y: i64, m: u32) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 => 29,
        2 => 28,
        _ => 0,
    }
}

struct Cursor<'a> {
    b: &'a [u8],
    i: usize,
}

impl Cursor<'_> {
    fn digits(&mut self, min: usize, max: usize) -> Option<u32> {
        let start = self.i;
        let mut v: u32 = 0;
        while self.i < self.b.len() && self.i - start < max && self.b[self.i].is_ascii_digit() {
            v = v * 10 + (self.b[self.i] - b'0') as u32;
            self.i += 1;
        }
        (self.i - start >= min).then_some(v)
    }
    fn eat(&mut self, c: u8) -> bool {
        if self.i < self.b.len() && self.b[self.i] == c {
            self.i += 1;
            true
        } else {
            false
        }
    }
    fn eat_any(&mut self, set: &[u8]) -> Option<u8> {
        if self.i < self.b.len() && set.contains(&self.b[self.i]) {
            self.i += 1;
            Some(self.b[self.i - 1])
        } else {
            None
        }
    }
    fn done(&self) -> bool {
        self.i >= self.b.len()
    }
    fn peek(&self) -> Option<u8> {
        self.b.get(self.i).copied()
    }
}

/// Interpreta datas e datas com hora. Devolve (dias desde 1970 + fração do dia, tem_hora).
///
/// Formatos: `2024-12-31`, `2024/12/31`, `31/12/2024`, `31-12-2024`, `31.12.2024`,
/// `31/12/24`, com hora opcional `HH:MM[:SS[.fff]]` (separada por espaço ou `T`)
/// e fuso opcional (`Z`, `+03:00`), que é ignorado.
pub fn parse_datetime(s: &str, order: DateOrder) -> Option<(f64, bool)> {
    let t = trim_ws(s);
    if t.len() < 6 || t.len() > 40 {
        return None;
    }
    let mut c = Cursor {
        b: t.as_bytes(),
        i: 0,
    };

    let first = c.digits(1, 4)?;
    let first_len = c.i;
    let sep = c.eat_any(b"-/.")?;
    let (y, m, d);
    if first_len == 4 {
        // Ano primeiro: AAAA-MM-DD
        let mm = c.digits(1, 2)?;
        if !c.eat(sep) {
            return None;
        }
        let dd = c.digits(1, 2)?;
        y = first as i64;
        m = mm;
        d = dd;
    } else {
        if first_len > 2 {
            return None;
        }
        let second = c.digits(1, 2)?;
        if !c.eat(sep) {
            return None;
        }
        let year_start = c.i;
        let yy = c.digits(2, 4)?;
        let year_len = c.i - year_start;
        if year_len == 3 {
            return None;
        }
        let year = if year_len == 2 {
            if yy < 70 { 2000 + yy } else { 1900 + yy }
        } else {
            yy
        } as i64;
        let (dd, mm) = match order {
            DateOrder::Dmy => (first, second),
            DateOrder::Mdy => (second, first),
        };
        y = year;
        m = mm;
        d = dd;
    }
    if !(1..=12).contains(&m) || d == 0 || d > days_in_month(y, m) || !(1000..=9999).contains(&y)
    {
        return None;
    }
    let days = days_from_civil(y, m, d) as f64;

    if c.done() {
        return Some((days, false));
    }
    // Hora
    if !(c.eat(b'T') || c.eat(b' ')) {
        return None;
    }
    while c.eat(b' ') {}
    let hh = c.digits(1, 2)?;
    if !c.eat(b':') {
        return None;
    }
    let mi = c.digits(2, 2)?;
    let mut ss = 0;
    let mut frac = 0.0;
    if c.eat(b':') {
        ss = c.digits(2, 2)?;
        if c.eat(b'.') || c.eat(b',') {
            let start = c.i;
            let v = c.digits(1, 9)?;
            frac = v as f64 / 10f64.powi((c.i - start) as i32);
        }
    }
    while c.eat(b' ') {}
    // AM/PM
    let mut hh = hh;
    if let Some(b) = c.peek()
        && matches!(b, b'A' | b'a' | b'P' | b'p')
    {
        let pm = matches!(b, b'P' | b'p');
        c.i += 1;
        if !(c.eat(b'M') || c.eat(b'm')) {
            return None;
        }
        if hh == 0 || hh > 12 {
            return None;
        }
        hh = match (pm, hh) {
            (false, 12) => 0,
            (true, 12) => 12,
            (true, h) => h + 12,
            (false, h) => h,
        };
    }
    // Fuso horário (ignorado)
    if c.eat(b'Z') || c.eat(b'z') {
    } else if c.eat_any(b"+-").is_some() {
        c.digits(2, 2)?;
        c.eat(b':');
        c.digits(0, 2);
    }
    if !c.done() || hh > 23 || mi > 59 || ss > 60 {
        return None;
    }
    let secs = hh as f64 * 3600.0 + mi as f64 * 60.0 + ss as f64 + frac;
    Some((days + secs / 86_400.0, true))
}

/// Descobre se datas com barra estão em dia/mês ou mês/dia.
fn detect_date_order<'a>(samples: impl Iterator<Item = &'a str>) -> DateOrder {
    let mut dmy = 0;
    let mut mdy = 0;
    for s in samples {
        let t = trim_ws(s);
        let mut parts = t.splitn(3, ['/', '-', '.']);
        let (Some(a), Some(b)) = (parts.next(), parts.next()) else {
            continue;
        };
        if a.len() > 2 {
            continue; // ano primeiro
        }
        let (Ok(a), Ok(b)) = (a.parse::<u32>(), b.parse::<u32>()) else {
            continue;
        };
        if a > 12 && b <= 12 {
            dmy += 1;
        } else if b > 12 && a <= 12 {
            mdy += 1;
        }
    }
    if mdy > dmy {
        DateOrder::Mdy
    } else {
        DateOrder::Dmy
    }
}

// ---------------------------------------------------------------------------
// Inferência por coluna

/// Resultado da inferência de tipo de uma coluna.
pub struct Inferred {
    pub kind: ColumnKind,
    /// Chave numérica por linha (NaN = vazio ou inválido).
    pub keys: Option<Vec<f64>>,
}

/// Decide o tipo de uma coluna e calcula as chaves numéricas.
///
/// `get` devolve o texto da célula `i`; `rows` é o número de linhas.
pub fn infer_column<'a>(
    rows: usize,
    get: impl Fn(usize) -> &'a str,
    decimal_fallback: DecimalSep,
) -> Inferred {
    // Amostra: até ~3000 valores não vazios espalhados pelo arquivo.
    const MAX_SAMPLE: usize = 3000;
    let stride = (rows / MAX_SAMPLE).max(1);
    let mut sample: Vec<&str> = Vec::with_capacity(MAX_SAMPLE.min(rows));
    let mut i = 0;
    while i < rows && sample.len() < MAX_SAMPLE * 2 {
        let v = get(i);
        if !is_null_token(v) {
            sample.push(v);
        }
        i += stride;
    }
    if sample.is_empty() {
        // Pode haver valores fora da amostra; procura um.
        if !(0..rows).any(|r| !is_null_token(get(r))) {
            return Inferred {
                kind: ColumnKind::Empty,
                keys: None,
            };
        }
        return Inferred {
            kind: ColumnKind::Text,
            keys: None,
        };
    }

    let n = sample.len() as f64;
    let ratio = |ok: usize| ok as f64 / n;

    // Booleano: precisa que (quase) todos sejam palavras sim/não.
    let bools = sample.iter().filter(|s| parse_bool(s).is_some()).count();
    if ratio(bools) >= 0.99 {
        let keys = compute_keys(rows, &get, |s| parse_bool(s).map(|b| b as u8 as f64));
        return finalize(ColumnKind::Bool, keys, rows, &get);
    }

    // Número
    let sep = detect_decimal_sep(sample.iter().copied(), decimal_fallback);
    let mut numeric_ok = 0;
    let mut has_fraction = false;
    for s in &sample {
        if let Some(v) = parse_number(s, sep) {
            numeric_ok += 1;
            if v.fract() != 0.0 || s.contains(match sep {
                DecimalSep::Dot => '.',
                DecimalSep::Comma => ',',
            }) {
                has_fraction = true;
            }
        }
    }
    if ratio(numeric_ok) >= 0.97 {
        // Códigos com zeros à esquerda (CEP, CPF sem máscara, IDs) continuam números,
        // mas são exibidos como estão no arquivo.
        let keys = compute_keys(rows, &get, |s| parse_number(s, sep));
        let kind = if has_fraction {
            ColumnKind::Decimal
        } else {
            ColumnKind::Integer
        };
        return finalize(kind, keys, rows, &get);
    }

    // Data
    let order = detect_date_order(sample.iter().copied());
    let mut date_ok = 0;
    let mut has_time = false;
    for s in &sample {
        if let Some((_, t)) = parse_datetime(s, order) {
            date_ok += 1;
            has_time |= t;
        }
    }
    if ratio(date_ok) >= 0.97 {
        let keys = compute_keys(rows, &get, |s| parse_datetime(s, order).map(|(v, _)| v));
        let kind = if has_time {
            ColumnKind::DateTime
        } else {
            ColumnKind::Date
        };
        return finalize(kind, keys, rows, &get);
    }

    Inferred {
        kind: ColumnKind::Text,
        keys: None,
    }
}

fn compute_keys<'a>(
    rows: usize,
    get: &impl Fn(usize) -> &'a str,
    parse: impl Fn(&str) -> Option<f64>,
) -> Vec<f64> {
    (0..rows)
        .map(|r| {
            let s = get(r);
            if s.is_empty() {
                f64::NAN
            } else {
                parse(s).unwrap_or(f64::NAN)
            }
        })
        .collect()
}

/// Confere o tipo contra todas as linhas: se muitos valores falharem fora da
/// amostra, a coluna volta a ser texto.
fn finalize<'a>(
    kind: ColumnKind,
    keys: Vec<f64>,
    rows: usize,
    get: &impl Fn(usize) -> &'a str,
) -> Inferred {
    let mut failed = 0usize;
    let mut filled = 0usize;
    for (r, k) in keys.iter().enumerate().take(rows) {
        if k.is_nan() {
            let s = get(r);
            if !is_null_token(s) {
                failed += 1;
            }
        } else {
            filled += 1;
        }
    }
    let total = failed + filled;
    if total > 0 && failed as f64 / total as f64 > 0.03 {
        return Inferred {
            kind: ColumnKind::Text,
            keys: None,
        };
    }
    Inferred {
        kind,
        keys: Some(keys),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeros_brasileiros() {
        let c = DecimalSep::Comma;
        assert_eq!(parse_number("1.234,56", c), Some(1234.56));
        assert_eq!(parse_number("R$ 1.234,56", c), Some(1234.56));
        assert_eq!(parse_number("-R$ 10,00", c), Some(-10.0));
        assert_eq!(parse_number("R$\u{a0}-3,5", c), Some(-3.5));
        assert_eq!(parse_number("12,5%", c), Some(12.5));
        assert_eq!(parse_number("(1.000,00)", c), Some(-1000.0));
        assert_eq!(parse_number("1.234.567", c), Some(1_234_567.0));
        assert_eq!(parse_number("12.34", c), None);
        assert_eq!(parse_number("123.456.789-09", c), None);
        assert_eq!(parse_number("(11) 98765-4321", c), None);
    }

    #[test]
    fn numeros_americanos() {
        let d = DecimalSep::Dot;
        assert_eq!(parse_number("1,234.56", d), Some(1234.56));
        assert_eq!(parse_number("$1,234", d), Some(1234.0));
        assert_eq!(parse_number("1.5e3", d), Some(1500.0));
        assert_eq!(parse_number(".5", d), Some(0.5));
        assert_eq!(parse_number("1.2.3", d), None);
        assert_eq!(parse_number("abc", d), None);
        assert_eq!(parse_number("1,23", d), None);
    }

    #[test]
    fn detecta_separador_decimal() {
        let br = ["1.234,56", "12,5", "3"];
        assert_eq!(
            detect_decimal_sep(br.iter().copied(), DecimalSep::Dot),
            DecimalSep::Comma
        );
        let us = ["1,234.56", "12.5", "3"];
        assert_eq!(
            detect_decimal_sep(us.iter().copied(), DecimalSep::Comma),
            DecimalSep::Dot
        );
    }

    #[test]
    fn datas() {
        let base = days_from_civil(2024, 12, 31) as f64;
        assert_eq!(parse_datetime("31/12/2024", DateOrder::Dmy), Some((base, false)));
        assert_eq!(parse_datetime("2024-12-31", DateOrder::Dmy), Some((base, false)));
        assert_eq!(parse_datetime("12/31/2024", DateOrder::Mdy), Some((base, false)));
        assert!(parse_datetime("31/02/2024", DateOrder::Dmy).is_none());
        let (v, t) = parse_datetime("2024-12-31T12:00:00Z", DateOrder::Dmy).unwrap();
        assert!(t);
        assert!((v - (base + 0.5)).abs() < 1e-9);
        let (v, _) = parse_datetime("31/12/2024 06:00 PM", DateOrder::Dmy).unwrap();
        assert!((v - (base + 0.75)).abs() < 1e-9);
        assert_eq!(civil_from_days(base as i64), (2024, 12, 31));
        assert!(parse_datetime("1.234,56", DateOrder::Dmy).is_none());
    }

    #[test]
    fn inferencia() {
        let vals = ["1.234,56", "12,5", "", "3", "N/A"];
        let inf = infer_column(vals.len(), |i| vals[i], DecimalSep::Dot);
        assert_eq!(inf.kind, ColumnKind::Decimal);
        let keys = inf.keys.unwrap();
        assert_eq!(keys[0], 1234.56);
        assert!(keys[2].is_nan());

        let vals = ["10", "20", "30"];
        assert_eq!(
            infer_column(vals.len(), |i| vals[i], DecimalSep::Dot).kind,
            ColumnKind::Integer
        );

        let vals = ["01/02/2024", "15/03/2024", ""];
        assert_eq!(
            infer_column(vals.len(), |i| vals[i], DecimalSep::Dot).kind,
            ColumnKind::Date
        );

        let vals = ["Sim", "Não", "sim"];
        assert_eq!(
            infer_column(vals.len(), |i| vals[i], DecimalSep::Dot).kind,
            ColumnKind::Bool
        );

        let vals = ["abc", "12", "xyz"];
        assert_eq!(
            infer_column(vals.len(), |i| vals[i], DecimalSep::Dot).kind,
            ColumnKind::Text
        );
    }
}
