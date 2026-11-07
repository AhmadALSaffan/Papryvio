# Evaluation

`papryvio-eval` (in `src-tauri/src/bin`) builds a throwaway library from a folder of PDFs with the real
pipeline (PDFium worker, chunking, FTS5, optional Ollama embeddings) and runs a JSONL dataset.

```bash
cd src-tauri
cargo run --bin papryvio-eval -- --dataset ../evaluation/datasets/smoke.jsonl --papers tests/fixtures --out ../evaluation/reports --chat-model qwen3:4b
```

Options: `--embed-model <name>` adds semantic and hybrid modes, `--chat-model <name>` asks every
question, `--k` (default 5), `--ollama <url>`, `--pdfium <path>`.

## Dataset format (one JSON object per line)

```json
{"id":"q1","kind":"direct","question":"…","paper":"file.pdf","pages":[6],"contains":"4.1 points","answerable":true}
```

`pages` are 1-based PDF pages that hold the evidence; `contains` optionally requires a phrase in the
retrieved passage. Use `answerable:false` for questions the library cannot answer. Suggested kinds:
`direct`, `paraphrase`, `exact_term`, `arabic_on_english`, `comparison`, `unanswerable`,
`conflicting`, `multi_passage`. Use 30–50 questions on papers you may use for evaluation, with a
separate test split.

## Metrics

| Metric | Definition |
|---|---|
| Hit@k | share of answerable questions with at least one relevant passage in the top k |
| Recall@k | share of the expected pages covered by relevant passages in the top k (capped at 1) |
| MRR | mean of 1 / rank of the first relevant passage |
| median / p95 ms | retrieval latency per question |
| answers | state, claims, structurally valid citations, quotes located on the page, dropped claims, unanswerable handled |

Structural citation checks prove links, not meaning. Answer correctness and citation support need a
human reviewer; record them alongside the report.

## Results measured so far

`evaluation/datasets/smoke.jsonl` — 4 questions over the generated two-page test fixture (a pipeline
smoke test, **not** a quality benchmark), Windows 11 x64, Ollama 0.40.14, `qwen3:4b`, no embedding model
installed (lexical only):

| Mode | Questions | Hit@5 | Recall@5 | MRR |
|---|---|---|---|---|
| Lexical | 3 | 1.000 | 1.000 | 1.000 |

Answers: the 3 answerable questions were answered with 1 valid citation each; exact quotes were located
for the 2 English questions (the Arabic question cited the right page with a paraphrased quote, so no
highlight); the unanswerable question returned `no_evidence`. Semantic and hybrid retrieval were tested
only with deterministic doubles in `rag_tests.rs`, because no embedding model was available locally.

## Package size (measured, release build, Windows x64)

| Item | Size |
|---|---|
| NSIS installer `Papryvio_0.1.0_x64-setup.exe` | 82.0 MiB |
| `papryvio.exe` (includes the embedded GitHub Copilot CLI from `github-copilot-sdk`'s `bundled-cli`) | 89.7 MB |
| `pdfium.dll` | 7.2 MB |
| Frontend (`dist`, fonts, pdf.js and its worker) | 7.9 MB |

Ollama models are not part of the package; their size and memory are separate.
