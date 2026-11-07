//! Evidence bundles, the structured answer contract and the citation validator.
//!
//! Structural validation proves that every citation points at evidence the attempt really
//! retrieved, on the right page, and (when quoted) that the quoted words are on that page.
//! It does not prove the claim is semantically supported — that is measured by evaluation.
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::ids::DocumentId;
use super::text::{locate, numbers_in};

pub const PROMPT_VERSION: i64 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Evidence {
    /// "E1", "E2", … — unique within one attempt.
    pub id: String,
    pub chunk_id: i64,
    pub document_id: DocumentId,
    pub generation_id: String,
    pub title: String,
    pub authors: Option<String>,
    pub year: Option<i32>,
    pub page_index: u32,
    /// Offset of `text` within the page text (chars).
    pub page_offset: u32,
    pub text: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct RawClaim {
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub evidence_ids: Vec<String>,
    #[serde(default)]
    pub quote: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct RawAnswer {
    #[serde(default)]
    pub answer_status: Option<String>,
    #[serde(default)]
    pub claims: Vec<RawClaim>,
    #[serde(default)]
    pub limitations: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum AnswerStatus {
    Answered,
    Partial,
    NoEvidence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ClaimKind {
    /// Directly stated in a cited passage.
    Stated,
    /// Papryvio's conclusion across cited passages; shown as synthesis, not quotation.
    Synthesis,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CitationRef {
    /// Display number within the answer (1-based).
    pub number: u32,
    pub evidence_id: String,
    pub document_id: DocumentId,
    pub page_index: u32,
    /// Char span within the page text when the quotation was located exactly.
    pub span: Option<(u32, u32)>,
    pub quote: Option<String>,
    /// Filled when persisted.
    pub citation_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ValidatedClaim {
    pub text: String,
    pub kind: ClaimKind,
    pub citations: Vec<CitationRef>,
    /// A number in the claim was not found in its cited passages.
    pub unverified_number: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ValidatedAnswer {
    pub status: AnswerStatus,
    pub claims: Vec<ValidatedClaim>,
    pub limitations: Option<String>,
    /// Claims removed because their citations did not check out.
    pub dropped_claims: u32,
}

/// Extracts the first JSON object from model output (tolerates code fences and prose).
pub fn parse_model_json(text: &str) -> Result<RawAnswer, String> {
    // Reasoning models may emit a <think>…</think> block before the answer.
    let text = match text.rfind("</think>") {
        Some(i) => &text[i + "</think>".len()..],
        None => text,
    };
    let start = text.find('{').ok_or("no JSON object in reply")?;
    let bytes = text.as_bytes();
    let mut depth = 0i32;
    let mut in_str = false;
    let mut escaped = false;
    let mut end = None;
    for (i, &b) in bytes.iter().enumerate().skip(start) {
        if in_str {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                in_str = false;
            }
            continue;
        }
        match b {
            b'"' => in_str = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    end = Some(i + 1);
                    break;
                }
            }
            _ => {}
        }
    }
    let end = end.ok_or("unterminated JSON object")?;
    serde_json::from_str(&text[start..end]).map_err(|e| format!("invalid answer JSON: {e}"))
}

/// Checks every claim against the attempt's evidence bundle.
pub fn validate_answer(raw: &RawAnswer, bundle: &[Evidence]) -> ValidatedAnswer {
    let mut claims = Vec::new();
    let mut dropped = 0u32;
    let mut number = 0u32;
    let mut numbered: Vec<(String, Option<(u32, u32)>, u32)> = Vec::new();
    for rc in &raw.claims {
        let text = rc.text.trim();
        if text.is_empty() {
            continue;
        }
        let kind = match rc.kind.as_deref() {
            Some("synthesis") => ClaimKind::Synthesis,
            _ => ClaimKind::Stated,
        };
        let mut cites = Vec::new();
        let mut cited_text = String::new();
        let mut seen = std::collections::HashSet::new();
        for eid in &rc.evidence_ids {
            let eid = eid.trim().trim_matches(|c| c == '[' || c == ']');
            if !seen.insert(eid.to_string()) {
                continue;
            }
            let Some(ev) = bundle.iter().find(|e| e.id == eid) else { continue };
            let span = rc
                .quote
                .as_deref()
                .filter(|q| !q.trim().is_empty())
                .and_then(|q| locate(&ev.text, q))
                .filter(|(_, _, unique)| *unique)
                .map(|(s, e, _)| (ev.page_offset + s as u32, ev.page_offset + e as u32));
            let quote = span.map(|(s, e)| {
                ev.text.chars().skip((s - ev.page_offset) as usize).take((e - s) as usize).collect::<String>()
            });
            // Same passage cited again keeps its number.
            let n = match numbered.iter().find(|(id, sp, _)| id == &ev.id && *sp == span) {
                Some((_, _, n)) => *n,
                None => {
                    number += 1;
                    numbered.push((ev.id.clone(), span, number));
                    number
                }
            };
            cited_text.push_str(&ev.text);
            cited_text.push('\n');
            cites.push(CitationRef {
                number: n,
                evidence_id: ev.id.clone(),
                document_id: ev.document_id.clone(),
                page_index: ev.page_index,
                span,
                quote,
                citation_id: None,
            });
        }
        if cites.is_empty() {
            dropped += 1;
            continue;
        }
        let source_numbers = numbers_in(&cited_text);
        let unverified_number = numbers_in(text).iter().any(|n| !source_numbers.contains(n));
        claims.push(ValidatedClaim { text: text.to_string(), kind, citations: cites, unverified_number });
    }
    let declared = raw.answer_status.as_deref().unwrap_or("answered");
    let status = if claims.is_empty() {
        AnswerStatus::NoEvidence
    } else if declared == "partial" || declared == "no_evidence" || dropped > 0 {
        AnswerStatus::Partial
    } else {
        AnswerStatus::Answered
    };
    let limitations = raw.limitations.as_ref().map(|l| l.trim().to_string()).filter(|l| !l.is_empty());
    ValidatedAnswer { status, claims, limitations, dropped_claims: dropped }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(id: &str, page: u32, offset: u32, text: &str) -> Evidence {
        Evidence {
            id: id.into(),
            chunk_id: 1,
            document_id: DocumentId::from_trusted("d1".into()),
            generation_id: "g".into(),
            title: "T".into(),
            authors: None,
            year: None,
            page_index: page,
            page_offset: offset,
            text: text.into(),
        }
    }

    #[test]
    fn parses_fenced_json() {
        let raw = parse_model_json("Sure!\n```json\n{\"answer_status\":\"answered\",\"claims\":[{\"text\":\"x {y}\",\"evidence_ids\":[\"E1\"]}]}\n```").unwrap();
        assert_eq!(raw.claims.len(), 1);
        assert!(parse_model_json("no json here").is_err());
        let thought = parse_model_json("<think>maybe {not this}</think>{\"claims\":[]}").unwrap();
        assert!(thought.claims.is_empty());
    }

    #[test]
    fn rejects_citations_outside_bundle_and_locates_quotes() {
        let bundle = vec![ev("E1", 5, 100, "Reranking improved top-10 accuracy by 4.1 points at k = 100.")];
        let raw = RawAnswer {
            answer_status: Some("answered".into()),
            claims: vec![
                RawClaim {
                    text: "Reranking improved accuracy by 4.1 points.".into(),
                    kind: Some("stated".into()),
                    evidence_ids: vec!["E1".into()],
                    quote: Some("improved top-10 accuracy by 4.1 points".into()),
                },
                RawClaim { text: "Made up.".into(), kind: None, evidence_ids: vec!["E9".into()], quote: None },
            ],
            limitations: None,
        };
        let v = validate_answer(&raw, &bundle);
        assert_eq!(v.claims.len(), 1);
        assert_eq!(v.dropped_claims, 1);
        assert_eq!(v.status, AnswerStatus::Partial);
        let c = &v.claims[0].citations[0];
        assert_eq!(c.page_index, 5);
        assert_eq!(c.number, 1);
        let (s, e) = c.span.unwrap();
        assert_eq!(s, 100 + 10);
        assert_eq!(c.quote.as_deref(), Some("improved top-10 accuracy by 4.1 points"));
        assert_eq!(e - s, 38);
        assert!(!v.claims[0].unverified_number);
    }

    #[test]
    fn flags_numbers_missing_from_sources_and_no_evidence() {
        let bundle = vec![ev("E1", 0, 0, "The gain was 4.1 points.")];
        let raw = RawAnswer {
            claims: vec![RawClaim { text: "The gain was 7.3 points.".into(), evidence_ids: vec!["E1".into()], ..Default::default() }],
            ..Default::default()
        };
        assert!(validate_answer(&raw, &bundle).claims[0].unverified_number);
        let empty = RawAnswer { answer_status: Some("no_evidence".into()), ..Default::default() };
        assert_eq!(validate_answer(&empty, &bundle).status, AnswerStatus::NoEvidence);
    }
}
