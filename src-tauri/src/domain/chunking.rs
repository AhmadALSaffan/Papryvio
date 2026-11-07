//! Page-bounded chunking. Chunks never cross a page so every citation maps to one page.
//! Sizes are starting values (≈400–800 tokens) to be tuned by evaluation.

pub const CHUNKER_VERSION: i64 = 1;
/// Characters per token used for the size *estimate* (no tokenizer for every model).
pub const CHARS_PER_TOKEN_ESTIMATE: usize = 4;
pub const TARGET_CHARS: usize = 2400; // ≈ 600 tokens
pub const MAX_CHARS: usize = 3200; // ≈ 800 tokens
pub const MIN_CHARS: usize = 400; // ≈ 100 tokens; smaller tails merge into the previous chunk

#[derive(Debug, Clone, PartialEq)]
pub struct PageChunk {
    /// Char offsets into the page text (end exclusive).
    pub start: usize,
    pub end: usize,
    pub text: String,
    pub token_estimate: usize,
}

pub fn estimate_tokens(text: &str) -> usize {
    text.chars().count().div_ceil(CHARS_PER_TOKEN_ESTIMATE)
}

/// Splits one page into chunks, respecting paragraph and sentence boundaries.
pub fn chunk_page(text: &str) -> Vec<PageChunk> {
    let chars: Vec<char> = text.chars().collect();
    // 1) Paragraph-ish units: split at blank lines, else at line breaks after sentence ends.
    let mut units: Vec<(usize, usize)> = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let para_break = c == '\n' && i + 1 < chars.len() && chars[i + 1] == '\n';
        let sentence_line = c == '\n' && i > 0 && matches!(chars[i - 1], '.' | '?' | '!' | '؟' | ':');
        if para_break || sentence_line {
            if i > start {
                units.push((start, i));
            }
            start = i + 1;
        }
        i += 1;
    }
    if start < chars.len() {
        units.push((start, chars.len()));
    }
    // 2) Oversized units are split at sentence boundaries close to the target.
    let mut pieces: Vec<(usize, usize)> = Vec::new();
    for (s, e) in units {
        let mut s = s;
        while e - s > MAX_CHARS {
            let window_end = s + TARGET_CHARS;
            let cut = (s + TARGET_CHARS / 2..window_end)
                .rev()
                .find(|&k| matches!(chars[k], '.' | '?' | '!' | '؟') && chars.get(k + 1).is_some_and(|n| n.is_whitespace()))
                .map(|k| k + 1)
                .or_else(|| (s + TARGET_CHARS / 2..window_end).rev().find(|&k| chars[k].is_whitespace()))
                .unwrap_or(window_end);
            pieces.push((s, cut));
            s = cut;
        }
        pieces.push((s, e));
    }
    // 3) Greedy packing up to the target size.
    let mut chunks: Vec<(usize, usize)> = Vec::new();
    for (s, e) in pieces {
        match chunks.last_mut() {
            Some(last) if e - last.0 <= TARGET_CHARS => last.1 = e,
            _ => chunks.push((s, e)),
        }
    }
    // 4) Merge a short tail into its predecessor when it still fits.
    if chunks.len() >= 2 {
        let (ts, te) = chunks[chunks.len() - 1];
        let (ps, _) = chunks[chunks.len() - 2];
        if te - ts < MIN_CHARS && te - ps <= MAX_CHARS {
            chunks.pop();
            chunks.last_mut().unwrap().1 = te;
        }
    }
    chunks
        .into_iter()
        .filter_map(|(s, e)| {
            // Trim surrounding whitespace but keep offsets exact.
            let mut s = s;
            let mut e = e;
            while s < e && chars[s].is_whitespace() {
                s += 1;
            }
            while e > s && chars[e - 1].is_whitespace() {
                e -= 1;
            }
            let text: String = chars[s..e].iter().collect();
            if text.chars().filter(|c| !c.is_whitespace()).count() < 20 {
                return None;
            }
            Some(PageChunk { start: s, end: e, token_estimate: estimate_tokens(&text), text })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_page_is_one_chunk() {
        let t = "Retrieval augmented generation grounds answers.\nEvery claim cites a page.";
        let c = chunk_page(t);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].text, t);
        assert_eq!((c[0].start, c[0].end), (0, t.chars().count()));
    }

    #[test]
    fn long_page_splits_within_limits_and_offsets_roundtrip() {
        let sentence = "Hybrid retrieval combines lexical and dense signals for recall. ";
        let para = sentence.repeat(30);
        let page = format!("{para}\n\n{para}\n\n{para}");
        let chars: Vec<char> = page.chars().collect();
        let chunks = chunk_page(&page);
        assert!(chunks.len() >= 3);
        for c in &chunks {
            assert!(c.text.chars().count() <= MAX_CHARS, "chunk too long: {}", c.text.len());
            let slice: String = chars[c.start..c.end].iter().collect();
            assert_eq!(slice, c.text);
        }
        // Coverage: chunks are ordered and do not overlap.
        for w in chunks.windows(2) {
            assert!(w[0].end <= w[1].start);
        }
    }

    #[test]
    fn empty_or_tiny_pages_produce_nothing() {
        assert!(chunk_page("").is_empty());
        assert!(chunk_page("Fig. 3").is_empty());
    }

    #[test]
    fn arabic_text_chunks() {
        let s = "يحسن الاسترجاع الهجين الاستدعاء عند دمج الإشارات المعجمية والدلالية. ".repeat(80);
        let chunks = chunk_page(&s);
        assert!(chunks.len() >= 2);
        assert!(chunks.iter().all(|c| c.token_estimate > 0));
    }
}
