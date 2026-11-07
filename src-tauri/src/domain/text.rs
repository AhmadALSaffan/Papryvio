//! Text normalization for search. The original text is always kept; these functions only
//! produce comparison/search copies.

/// Lowercases and applies limited Arabic normalization: removes diacritics and tatweel,
/// unifies alef/ya/ta-marbuta forms. English terms, numbers and units are preserved.
pub fn normalize_for_search(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\u{064B}'..='\u{065F}' | '\u{0670}' | '\u{0640}' => {}
            'أ' | 'إ' | 'آ' | 'ٱ' => out.push('ا'),
            'ى' => out.push('ي'),
            'ة' => out.push('ه'),
            'ؤ' => out.push('و'),
            'ئ' => out.push('ي'),
            _ => out.extend(c.to_lowercase()),
        }
    }
    out
}

fn is_arabic_letter(c: char) -> bool {
    matches!(c, '\u{0621}'..='\u{064A}' | '\u{066E}'..='\u{06D3}' | '\u{06FA}'..='\u{06FF}' | '\u{0750}'..='\u{077F}' | '\u{FB50}'..='\u{FDFF}' | '\u{FE70}'..='\u{FEFC}')
}

fn is_arabic_mark(c: char) -> bool {
    matches!(c, '\u{0610}'..='\u{061A}' | '\u{064B}'..='\u{065F}' | '\u{0670}' | '\u{06D6}'..='\u{06ED}')
}

/// Arabic words (letters only, marks dropped) of at least 3 letters.
fn arabic_words(s: &str) -> impl Iterator<Item = Vec<char>> + '_ {
    s.split(|c: char| !is_arabic_letter(c) && !is_arabic_mark(c))
        .map(|w| w.chars().filter(|c| is_arabic_letter(*c)).collect::<Vec<_>>())
        .filter(|w| w.len() >= 3)
}

/// Some PDFs store Arabic glyphs in visual order, so extracted words come out letter-reversed
/// ("ةيلصألا" for "الأصلية"). Returns `Some(true)` when a text clearly reads reversed,
/// `Some(false)` when it clearly reads in logical order, `None` when there is too little Arabic.
pub fn arabic_reversed(s: &str) -> Option<bool> {
    let (mut logical, mut reversed) = (0u32, 0u32);
    for w in arabic_words(s) {
        let (first, last) = (w[0], w[w.len() - 1]);
        if w[0] == 'ا' && w[1] == 'ل' {
            logical += 1;
        }
        if matches!(last, 'ة' | 'ى') {
            logical += 1;
        }
        if w.len() >= 4 && w[w.len() - 2] == 'ل' && last == 'ا' {
            reversed += 1;
        }
        if matches!(first, 'ة' | 'ى') {
            reversed += 1;
        }
    }
    if reversed >= 3 && reversed > logical * 2 {
        Some(true)
    } else if logical >= 3 && logical > reversed * 2 {
        Some(false)
    } else {
        None
    }
}

/// Reverses the letters of every unmarked Arabic word; everything else (spacing, punctuation, Latin, digits) in place. The char count is unchanged,
/// so offsets into the text stay valid.
pub fn unreverse_arabic(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        if !is_arabic_letter(chars[i]) {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        let mut clusters: Vec<&[char]> = Vec::new();
        while i < chars.len() && is_arabic_letter(chars[i]) {
            let start = i;
            i += 1;
            while i < chars.len() && is_arabic_mark(chars[i]) {
                i += 1;
            }
            clusters.push(&chars[start..i]);
        }
        // Words carrying vowel marks come from shaped runs whose order is not a plain reversal
        // (pronunciation guides in some PDFs); they are left as extracted.
        if clusters.iter().any(|c| c.len() > 1) {
            clusters.iter().for_each(|c| out.extend(c.iter()));
        } else {
            clusters.iter().rev().for_each(|c| out.extend(c.iter()));
        }
    }
    out
}

/// Strips attached Arabic article/preposition prefixes ("ال", "وال", "بال", "لل"…) from a
/// normalized token so "بالصفحه" and "الصفحه" both index as "صفحه".
pub fn strip_arabic_prefix(t: &str) -> &str {
    if !t.chars().next().is_some_and(is_arabic_letter) {
        return t;
    }
    for p in ["وال", "بال", "كال", "فال", "لل", "ال"] {
        if let Some(rest) = t.strip_prefix(p) {
            if rest.chars().count() >= 2 {
                return rest;
            }
        }
    }
    t
}

/// The copy of a chunk that the keyword index stores: search normalization plus Arabic
/// prefix stripping, applied identically to questions (see `retrieval::fts_query`).
pub fn search_text(s: &str) -> String {
    let normalized = normalize_for_search(s);
    let mut out = String::with_capacity(normalized.len());
    let mut word = String::new();
    for c in normalized.chars().chain(std::iter::once(' ')) {
        if c.is_alphanumeric() {
            word.push(c);
        } else {
            out.push_str(strip_arabic_prefix(&word));
            word.clear();
            out.push(c);
        }
    }
    out.pop();
    out
}

/// Normalization used to compare a model's quotation with the source text: search
/// normalization, typographic quotes/dashes/ligatures unified, hyphenated line breaks joined,
/// whitespace collapsed. Returns the normalized string and, for each of its chars, the index
/// of the originating char in `s` (so a match can be mapped back to the source).
pub fn normalize_for_match(s: &str) -> (String, Vec<usize>) {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut map = Vec::new();
    let mut last_space = true;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        // "retrie-\nval" → "retrieval"
        if c == '-' && i + 1 < chars.len() && chars[i + 1] == '\n' {
            i += 2;
            continue;
        }
        let mapped: Option<&str> = match c {
            '\u{2018}' | '\u{2019}' | '\u{201B}' | '`' => Some("'"),
            '\u{201C}' | '\u{201D}' | '\u{00AB}' | '\u{00BB}' => Some("\""),
            '\u{2013}' | '\u{2014}' | '\u{2212}' => Some("-"),
            '\u{FB01}' => Some("fi"),
            '\u{FB02}' => Some("fl"),
            '\u{00A0}' => Some(" "),
            _ => None,
        };
        let piece: String = match mapped {
            Some(m) => m.to_string(),
            None => normalize_for_search(&c.to_string()),
        };
        for pc in piece.chars() {
            if pc.is_whitespace() {
                if !last_space {
                    out.push(' ');
                    map.push(i);
                    last_space = true;
                }
            } else {
                out.push(pc);
                map.push(i);
                last_space = false;
            }
        }
        i += 1;
    }
    if out.ends_with(' ') {
        out.pop();
        map.pop();
    }
    (out, map)
}

/// Finds `needle` in `haystack` after match-normalization. Returns the char range in the
/// original haystack, and whether the match was unique.
pub fn locate(haystack: &str, needle: &str) -> Option<(usize, usize, bool)> {
    let (h, map) = normalize_for_match(haystack);
    let (n, _) = normalize_for_match(needle);
    let n = n.trim_matches(|c: char| c == '"' || c == '\'' || c == ' ' || c == '.' || c == '…');
    if n.chars().count() < 8 {
        return None;
    }
    let h_chars: Vec<char> = h.chars().collect();
    let n_chars: Vec<char> = n.chars().collect();
    let mut found: Option<usize> = None;
    let mut unique = true;
    let mut i = 0;
    while i + n_chars.len() <= h_chars.len() {
        if h_chars[i..i + n_chars.len()] == n_chars[..] {
            if found.is_some() {
                unique = false;
                break;
            }
            found = Some(i);
        }
        i += 1;
    }
    let start = found?;
    let end = start + n_chars.len() - 1;
    Some((map[start], map[end] + 1, unique))
}

/// Numbers (with decimals) appearing in a text, normalized ("4.1", "100", "0,31" → "0.31").
pub fn numbers_in(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let chars: Vec<char> = s.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        let d = match c {
            '0'..='9' => Some(c),
            '\u{0660}'..='\u{0669}' => char::from_u32('0' as u32 + (c as u32 - 0x0660)),
            _ => None,
        };
        if let Some(d) = d {
            cur.push(d);
        } else if (c == '.' || c == ',') && !cur.is_empty() && chars.get(i + 1).is_some_and(|n| n.is_ascii_digit()) {
            cur.push('.');
        } else if !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arabic_normalization() {
        assert_eq!(normalize_for_search("إعادةُ الترتيب"), "اعاده الترتيب");
        assert_eq!(normalize_for_search("Hybrid BM25"), "hybrid bm25");
        assert_eq!(normalize_for_search("مُسْتَـــشفى"), "مستشفي");
    }

    #[test]
    fn reversed_arabic_is_detected_and_repaired() {
        // Extracted from a real PDF that stores Arabic in visual order.
        let page = "لخادملا ةيلصألا  0005 - 0001\nقطن ةملكلا  كَنِنشيَ\nىنعملا  عبطلاب\nةمجرتلا  انأ بعتم، كلذل دوعأس ىلإ لزنملا.";
        assert_eq!(arabic_reversed(page), Some(true));
        let fixed = unreverse_arabic(page);
        assert_eq!(fixed.chars().count(), page.chars().count());
        assert!(fixed.starts_with("المداخل الأصلية  0005 - 0001\nنطق الكلمة"), "{fixed}");
        assert!(fixed.contains("أنا متعب، لذلك سأعود إلى المنزل."), "{fixed}");
        assert_eq!(arabic_reversed(&fixed), Some(false));
        assert_eq!(unreverse_arabic("Hello 42"), "Hello 42");
        assert_eq!(arabic_reversed("Only English text here."), None);
    }

    #[test]
    fn search_text_strips_arabic_prefixes() {
        assert_eq!(search_text("نطق الكلمة بالصفحة، Page 5"), "نطق كلمه صفحه، page 5");
        assert_eq!(strip_arabic_prefix("الا"), "الا");
        assert_eq!(strip_arabic_prefix("للبحث"), "بحث");
    }

    #[test]
    fn locate_handles_typography_and_hyphenation() {
        let page = "We found that “reranking im-\nproved top-10 accuracy by 4.1 points” at k = 100.";
        let (s, e, unique) = locate(page, "reranking improved top-10 accuracy by 4.1 points").unwrap();
        assert!(unique);
        let got: String = page.chars().skip(s).take(e - s).collect();
        assert!(got.starts_with("reranking") && got.ends_with("points"), "{got}");
        assert!(locate(page, "not in the page at all").is_none());
        assert!(locate(page, "short").is_none());
    }

    #[test]
    fn duplicate_matches_are_not_unique() {
        let page = "the gain was small. the gain was small.";
        assert_eq!(locate(page, "the gain was small").map(|m| m.2), Some(false));
    }

    #[test]
    fn numbers() {
        assert_eq!(numbers_in("improved by 4.1 points at k = 100 (p = 0,31)."), vec!["4.1", "100", "0.31"]);
        assert_eq!(numbers_in("تحسن بمقدار ٤٫١"), vec!["4", "1"]);
        assert_eq!(numbers_in("no digits"), Vec::<String>::new());
    }
}
