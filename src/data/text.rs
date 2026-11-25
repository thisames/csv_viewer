//! Utilitários de texto: busca sem diferenciar maiúsculas/acentos e comparação
//! "humana" para ordenação.

use std::cmp::Ordering;

/// Converte um caractere para a forma usada em buscas e ordenação:
/// minúsculo e sem acento (`'Ã'` → `'a'`, `'Ç'` → `'c'`).
///
/// Sempre devolve exatamente um caractere, o que permite mapear posições do
/// texto "dobrado" de volta para o texto original.
#[inline]
pub fn fold_char(c: char) -> char {
    if c.is_ascii() {
        return c.to_ascii_lowercase();
    }
    match c {
        'À' | 'Á' | 'Â' | 'Ã' | 'Ä' | 'Å' | 'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'Ā' | 'ā' | 'Ă'
        | 'ă' | 'Ą' | 'ą' | 'ª' => 'a',
        'Ç' | 'ç' | 'Ć' | 'ć' | 'Č' | 'č' => 'c',
        'È' | 'É' | 'Ê' | 'Ë' | 'è' | 'é' | 'ê' | 'ë' | 'Ē' | 'ē' | 'Ę' | 'ę' | 'Ě' | 'ě' => 'e',
        'Ì' | 'Í' | 'Î' | 'Ï' | 'ì' | 'í' | 'î' | 'ï' | 'Ī' | 'ī' => 'i',
        'Ñ' | 'ñ' | 'Ń' | 'ń' | 'Ň' | 'ň' => 'n',
        'Ò' | 'Ó' | 'Ô' | 'Õ' | 'Ö' | 'Ø' | 'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'Ō' | 'ō' | 'º' => {
            'o'
        }
        'Ù' | 'Ú' | 'Û' | 'Ü' | 'ù' | 'ú' | 'û' | 'ü' | 'Ū' | 'ū' | 'Ů' | 'ů' => 'u',
        'Ý' | 'ý' | 'ÿ' | 'Ÿ' => 'y',
        'Š' | 'š' | 'Ś' | 'ś' | 'ß' => 's',
        'Ž' | 'ž' | 'Ź' | 'ź' | 'Ż' | 'ż' => 'z',
        'Ł' | 'ł' => 'l',
        'Đ' | 'đ' | 'Ď' | 'ď' => 'd',
        'Ř' | 'ř' => 'r',
        'Ť' | 'ť' => 't',
        _ => {
            let mut lower = c.to_lowercase();
            match (lower.next(), lower.next()) {
                (Some(l), None) => l,
                _ => c,
            }
        }
    }
}

/// Texto de busca já normalizado (minúsculo, sem acentos).
#[derive(Clone, Debug, Default)]
pub struct Needle {
    chars: Vec<char>,
    /// Versão em bytes quando a agulha é puramente ASCII (caminho rápido).
    ascii: Option<Vec<u8>>,
}

impl Needle {
    pub fn new(query: &str) -> Self {
        let chars: Vec<char> = query.trim().chars().map(fold_char).collect();
        let ascii = chars
            .iter()
            .all(char::is_ascii)
            .then(|| chars.iter().map(|&c| c as u8).collect());
        Self { chars, ascii }
    }

    pub fn is_empty(&self) -> bool {
        self.chars.is_empty()
    }

    /// O texto contém a agulha?
    pub fn matches(&self, hay: &str) -> bool {
        if self.chars.is_empty() {
            return true;
        }
        if hay.len() < self.chars.len() {
            // Cada caractere ocupa pelo menos 1 byte.
            return false;
        }
        if let Some(needle) = &self.ascii
            && hay.is_ascii()
        {
            return ascii_contains_ignore_case(hay.as_bytes(), needle);
        }
        self.find_from(hay, 0).is_some()
    }

    /// Todas as ocorrências (intervalos em bytes do texto original), sem sobreposição.
    pub fn find_all(&self, hay: &str) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        if self.chars.is_empty() {
            return out;
        }
        let mut from = 0;
        while let Some((s, e)) = self.find_from(hay, from) {
            out.push((s, e));
            from = e;
            if out.len() > 64 {
                break;
            }
        }
        out
    }

    fn find_from(&self, hay: &str, from: usize) -> Option<(usize, usize)> {
        let rest = hay.get(from..)?;
        let first = self.chars[0];
        for (i, c) in rest.char_indices() {
            if fold_char(c) != first {
                continue;
            }
            let mut it = rest[i..].char_indices();
            let mut end = i;
            let mut ok = true;
            for &nc in &self.chars {
                match it.next() {
                    Some((j, hc)) if fold_char(hc) == nc => end = i + j + hc.len_utf8(),
                    _ => {
                        ok = false;
                        break;
                    }
                }
            }
            if ok {
                return Some((from + i, from + end));
            }
        }
        None
    }
}

/// Busca ASCII sem diferenciar maiúsculas. `needle` já deve estar em minúsculas.
fn ascii_contains_ignore_case(hay: &[u8], needle: &[u8]) -> bool {
    let n = needle.len();
    if n == 0 {
        return true;
    }
    if hay.len() < n {
        return false;
    }
    let first_lo = needle[0];
    let first_up = first_lo.to_ascii_uppercase();
    let last_start = hay.len() - n;
    let mut pos = 0;
    while pos <= last_start {
        let found = if first_lo == first_up {
            memchr::memchr(first_lo, &hay[pos..=last_start])
        } else {
            memchr::memchr2(first_lo, first_up, &hay[pos..=last_start])
        };
        let Some(off) = found else { return false };
        let start = pos + off;
        if hay[start + 1..start + n]
            .iter()
            .zip(&needle[1..])
            .all(|(h, nb)| h.to_ascii_lowercase() == *nb)
        {
            return true;
        }
        pos = start + 1;
    }
    false
}

/// Comparação de texto para ordenação: ignora maiúsculas e acentos
/// (desempate pelo texto original, para a ordem ser determinística).
pub fn cmp_text(a: &str, b: &str) -> Ordering {
    let primary = if a.is_ascii() && b.is_ascii() {
        a.bytes()
            .map(|c| c.to_ascii_lowercase())
            .cmp(b.bytes().map(|c| c.to_ascii_lowercase()))
    } else {
        a.chars().map(fold_char).cmp(b.chars().map(fold_char))
    };
    primary.then_with(|| a.cmp(b))
}

/// Corta um texto em no máximo `max_chars` caracteres (respeitando UTF-8).
pub fn truncate_chars(s: &str, max_chars: usize) -> &str {
    match s.char_indices().nth(max_chars) {
        Some((i, _)) => &s[..i],
        None => s,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn busca_ignora_acentos_e_maiusculas() {
        let n = Needle::new("sao paulo");
        assert!(n.matches("Cidade de São Paulo"));
        assert!(n.matches("SAO PAULO"));
        assert!(!n.matches("Santos"));
        let n = Needle::new("CONCEIÇÃO");
        assert!(n.matches("rua da conceicao"));
    }

    #[test]
    fn find_all_retorna_intervalos_do_texto_original() {
        let n = Needle::new("ão");
        let hay = "Pão e Feijão";
        let found = n.find_all(hay);
        assert_eq!(found.len(), 2);
        assert_eq!(&hay[found[0].0..found[0].1], "ão");
        assert_eq!(&hay[found[1].0..found[1].1], "ão");
    }

    #[test]
    fn ascii_rapido() {
        let n = Needle::new("ABC");
        assert!(n.matches("xxabcxx"));
        assert!(n.matches("xxAbC"));
        assert!(!n.matches("ab"));
        assert!(!n.matches("xxabxcx"));
    }

    #[test]
    fn ordenacao_humana() {
        assert_eq!(cmp_text("álvaro", "Bruno"), Ordering::Less);
        assert_eq!(cmp_text("Zé", "ana"), Ordering::Greater);
    }
}
