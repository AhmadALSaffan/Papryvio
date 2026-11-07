//! Retrieval rules: safe FTS5 query construction, vector checks and Reciprocal Rank Fusion.
use std::collections::HashMap;

use super::text::{normalize_for_search, strip_arabic_prefix};

pub const RRF_K: f64 = 60.0;
pub const CANDIDATES_PER_PATH: usize = 20;
pub const MAX_PER_DOCUMENT: usize = 4;

const STOPWORDS: &[&str] = &[
    "the", "a", "an", "and", "or", "of", "to", "in", "on", "for", "with", "is", "are", "was", "were", "be", "by",
    "this", "that", "these", "those", "it", "as", "at", "from", "what", "which", "how", "does", "do", "did", "paper",
    "papers", "me", "my", "i", "you", "can", "tell", "about", "please", "there", "here", "who", "why", "when", "where",
    // Arabic (normalized forms: ى→ي, ة→ه, أ/إ/آ→ا), including common spoken forms.
    "في", "من", "علي", "الي", "عن", "ما", "ماذا", "هل", "التي", "الذي", "هذه", "هذا", "مع", "او", "و", "هي", "هو",
    "هيه", "هوه", "ايش", "شو", "شنو", "وش", "ليش", "لماذا", "وين", "اين", "كيف", "كم", "اللي", "هاي", "هاد",
    "هذي", "هدا", "انا", "انت", "بدي", "اريد", "ابي", "ابغي", "ممكن", "يعني", "لي", "فيها", "فيه", "عندي", "اعطني",
    "اشرح", "وضح", "قل", "قلي", "لو", "سمحت", "رجاء", "كان", "كانت", "يكون", "تكون", "ذلك", "تلك", "هناك", "هنا",
    "ايه", "ازاي", "دي", "ده",
];

/// Arabic suffixes dropped from question terms, which then match as prefixes ("الكلمات" → كلم*).
const AR_SUFFIXES: &[&str] = &["ات", "ون", "ين", "ها", "هم", "ه", "ي"];

fn is_arabic(t: &str) -> bool {
    t.chars().next().is_some_and(|c| ('\u{0621}'..='\u{06FF}').contains(&c))
}

fn quoted(t: &str) -> String {
    format!("\"{}\"", t.replace('"', "\"\""))
}

/// Builds an FTS5 MATCH expression from free text: every term is quoted, so user input
/// can never inject FTS operators. Arabic terms lose attached prefixes and plural/pronoun
/// suffixes and match as prefixes, mirroring `text::search_text` on the indexed side.
/// Returns `None` when nothing searchable remains.
pub fn fts_query(question: &str) -> Option<String> {
    let normalized = normalize_for_search(question);
    let mut terms: Vec<String> = Vec::new();
    for raw in normalized.split(|c: char| !c.is_alphanumeric()) {
        let keep = raw.chars().count() >= 2 || (!raw.is_empty() && raw.chars().all(|c| c.is_ascii_digit()));
        if !keep || STOPWORDS.contains(&raw) {
            continue;
        }
        let term = if is_arabic(raw) {
            let base = strip_arabic_prefix(raw);
            let stem = AR_SUFFIXES
                .iter()
                .find_map(|s| base.strip_suffix(s).filter(|r| r.chars().count() >= 3))
                .unwrap_or(base);
            if stem.chars().count() >= 3 { format!("{}*", quoted(stem)) } else { quoted(stem) }
        } else {
            quoted(raw)
        };
        if !terms.contains(&term) {
            terms.push(term);
        }
    }
    if terms.is_empty() {
        return None;
    }
    terms.truncate(32);
    Some(terms.join(" OR "))
}

/// L2-normalizes a vector after checking it is usable (finite, non-zero, expected size).
pub fn normalize_vector(v: &[f32], expected_dims: Option<usize>) -> Result<Vec<f32>, &'static str> {
    if v.is_empty() {
        return Err("empty vector");
    }
    if let Some(d) = expected_dims {
        if v.len() != d {
            return Err("dimension mismatch");
        }
    }
    if v.iter().any(|x| !x.is_finite()) {
        return Err("non-finite value");
    }
    let norm = v.iter().map(|x| (*x as f64) * (*x as f64)).sum::<f64>().sqrt();
    if norm < 1e-12 {
        return Err("zero vector");
    }
    Ok(v.iter().map(|x| (*x as f64 / norm) as f32).collect())
}

pub fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

pub fn vector_to_blob(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

pub fn blob_to_vector(b: &[u8]) -> Vec<f32> {
    b.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect()
}

/// score(d) = Σ 1 / (k + rank_i(d)), ranks starting at 1. Ties break by best single rank,
/// then by id, so the fusion is deterministic.
pub fn reciprocal_rank_fusion(lists: &[&[i64]], k: f64) -> Vec<(i64, f64)> {
    let mut score: HashMap<i64, f64> = HashMap::new();
    let mut best: HashMap<i64, usize> = HashMap::new();
    for list in lists {
        let mut seen = std::collections::HashSet::new();
        for (i, id) in list.iter().enumerate() {
            if !seen.insert(*id) {
                continue; // duplicates inside one list count once
            }
            let rank = i + 1;
            *score.entry(*id).or_default() += 1.0 / (k + rank as f64);
            let b = best.entry(*id).or_insert(rank);
            *b = (*b).min(rank);
        }
    }
    let mut out: Vec<(i64, f64)> = score.into_iter().collect();
    out.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(best[&a.0].cmp(&best[&b.0]))
            .then(a.0.cmp(&b.0))
    });
    out
}

/// Keeps at most `max_per_doc` results per document, preserving order.
pub fn cap_per_document<T: Clone>(items: &[T], doc_of: impl Fn(&T) -> &str, max_per_doc: usize) -> Vec<T> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    items
        .iter()
        .filter(|it| {
            let c = counts.entry(doc_of(it).to_string()).or_default();
            *c += 1;
            *c <= max_per_doc
        })
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fts_query_quotes_terms_and_drops_operators() {
        assert_eq!(fts_query("Which papers tested reranking?").unwrap(), "\"papers\" OR \"tested\" OR \"reranking\"".replace("\"papers\" OR ", ""));
        let q = fts_query("BM25 OR NEAR(\"x\") -top-10 k=20").unwrap();
        assert!(!q.contains("NEAR("));
        assert!(q.split(" OR ").all(|t| t.starts_with('"') && t.ends_with('"')));
        assert!(fts_query("?? ... !!").is_none());
        assert_eq!(fts_query("ما هي إعادة الترتيب").unwrap(), "\"اعاد\"* OR \"ترتيب\"*");
        assert_eq!(fts_query("ايش هية الكلمات الي بالصفحة؟").unwrap(), "\"كلم\"* OR \"صفح\"*");
    }

    #[test]
    fn vector_checks() {
        assert!(normalize_vector(&[0.0, 0.0], None).is_err());
        assert!(normalize_vector(&[1.0, f32::NAN], None).is_err());
        assert!(normalize_vector(&[1.0, 2.0], Some(3)).is_err());
        let v = normalize_vector(&[3.0, 4.0], Some(2)).unwrap();
        assert!((dot(&v, &v) - 1.0).abs() < 1e-6);
        assert_eq!(blob_to_vector(&vector_to_blob(&v)), v);
    }

    #[test]
    fn rrf_is_deterministic_and_dedupes() {
        let lexical = [1, 2, 3, 2];
        let semantic = [3, 4, 1];
        let fused = reciprocal_rank_fusion(&[&lexical, &semantic], RRF_K);
        let ids: Vec<i64> = fused.iter().map(|x| x.0).collect();
        // 1: 1/61 + 1/63, 3: 1/63 + 1/61 → tie broken by best rank (both 1) then id.
        assert_eq!(ids, vec![1, 3, 2, 4]);
        let expected = 1.0 / 61.0 + 1.0 / 63.0;
        assert!((fused[0].1 - expected).abs() < 1e-12);
    }

    #[test]
    fn per_document_cap() {
        let items = vec![("a", 1), ("a", 2), ("b", 3), ("a", 4)];
        let capped = cap_per_document(&items, |x| x.0, 2);
        assert_eq!(capped, vec![("a", 1), ("a", 2), ("b", 3)]);
    }
}
