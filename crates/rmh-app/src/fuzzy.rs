//! Typo-tolerant matching for search ("intersteler" → Interstellar,
//! "pelisky" → Pelíšky).

/// Lower-case, strip diacritics and punctuation, collapse spaces
pub fn normalize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut space = true;
    for c in s.chars().flat_map(char::to_lowercase) {
        let c = match c {
            'á' | 'à' | 'â' | 'ä' | 'ã' | 'å' | 'ą' => 'a',
            'č' | 'ç' | 'ć' => 'c',
            'ď' => 'd',
            'é' | 'è' | 'ê' | 'ë' | 'ě' | 'ę' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ľ' | 'ĺ' | 'ł' => 'l',
            'ň' | 'ñ' | 'ń' => 'n',
            'ó' | 'ò' | 'ô' | 'ö' | 'õ' | 'ø' => 'o',
            'ř' | 'ŕ' => 'r',
            'š' | 'ś' | 'ß' => 's',
            'ť' => 't',
            'ú' | 'ù' | 'û' | 'ü' | 'ů' => 'u',
            'ý' | 'ÿ' => 'y',
            'ž' | 'ź' | 'ż' => 'z',
            '&' => ' ',
            c => c,
        };
        if c.is_alphanumeric() {
            out.push(c);
            space = false;
        } else if !space {
            out.push(' ');
            space = true;
        }
    }
    out.trim_end().to_string()
}

/// Levenshtein distance with adjacent transpositions (OSA)
pub fn distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let (n, m) = (a.len(), b.len());
    if n == 0 {
        return m;
    }
    if m == 0 {
        return n;
    }
    let mut d = vec![vec![0usize; m + 1]; n + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for j in 0..=m {
        d[0][j] = j;
    }
    for i in 1..=n {
        for j in 1..=m {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            d[i][j] = (d[i - 1][j] + 1).min(d[i][j - 1] + 1).min(d[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                d[i][j] = d[i][j].min(d[i - 2][j - 2] + 1);
            }
        }
    }
    d[n][m]
}

fn token_sim(q: &str, t: &str) -> f32 {
    if t.starts_with(q) {
        return 1.0;
    }
    let ql = q.chars().count();
    if ql < 3 {
        return if t == q { 1.0 } else { 0.0 };
    }
    // compare against the whole word and against its prefix of the same
    // length, so half-typed words with a typo still match
    let prefix: String = t.chars().take(ql).collect();
    let d = distance(q, t).min(distance(q, &prefix));
    (1.0 - d as f32 / ql.max(1) as f32).max(0.0)
}

/// 0.0–1.0: how well `query` matches `text`
pub fn score(query: &str, text: &str) -> f32 {
    let q = normalize(query);
    let t = normalize(text);
    if q.is_empty() || t.is_empty() {
        return 0.0;
    }
    if t == q {
        return 1.0;
    }
    if t.starts_with(&q) {
        return 0.97;
    }
    if t.contains(&format!(" {q}")) {
        return 0.93;
    }
    if q.chars().count() >= 3 && t.contains(&q) {
        return 0.88;
    }
    let tt: Vec<&str> = t.split(' ').collect();
    let qt: Vec<&str> = q.split(' ').collect();
    let per: f32 = qt.iter().map(|q| tt.iter().map(|t| token_sim(q, t)).fold(0.0, f32::max)).sum::<f32>() / qt.len() as f32;
    let whole = 1.0 - distance(&q, &t) as f32 / q.chars().count().max(t.chars().count()) as f32;
    (per * 0.9).max(whole).clamp(0.0, 0.87)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typos_and_diacritics() {
        assert_eq!(normalize("Pelíšky (1999)"), "pelisky 1999");
        assert_eq!(distance("intersteller", "interstellar"), 1);
        assert_eq!(distance("teh", "the"), 1);
        assert!(score("pelisky", "Pelíšky") > 0.95);
        assert!(score("intersteler", "Interstellar") > 0.75);
        assert!(score("expanse", "The Expanse") > 0.9);
        assert!(score("brekaing bad", "Breaking Bad") > 0.75);
        assert!(score("dun", "Dune") > 0.9);
        assert!(score("matrix", "Dune") < 0.5);
        assert!(score("game of thornes", "Game of Thrones") > 0.8);
        assert!(score("x", "Dune") < 0.5);
    }
}
