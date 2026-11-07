# Papryvio — architecture

Modular monolith in one Rust crate (`src-tauri`) with Clean/Hexagonal layers, plus a React/TypeScript UI.

```
React UI ──invoke/events──▶ desktop (Tauri commands, IPC DTOs, events)
                                  │
                                  ▼
                       application (use cases + ports)
                         │                      │
                         ▼                      ▼
                     domain rules        infrastructure adapters
                                 SQLite+FTS5 · file store · PDFium worker
                                 Ollama · OpenAI-compatible · Anthropic
                                 ChatGPT plan · GitHub Copilot SDK · OS credential store
```

`bootstrap.rs` is the only place that knows concrete adapters.

## Flow of a question

1. **Scope** (`application/retrieval::resolve_scope`) — collection or chosen papers. Only papers with a
   published index generation are searched; the others are reported as *not searched* with a reason
   (`processing`, `needs_ocr`, `failed`, `no_vectors`).
2. **Retrieval** — keyword: FTS5 over a normalized copy of each chunk (Arabic letter forms unified),
   BM25 ranking, every query term quoted so user text cannot inject FTS operators. Meaning: the
   question is embedded once with the *same* embedding model fingerprint as the index; cosine on
   L2-normalized float32 vectors with a bounded heap. **Hybrid**: Reciprocal Rank Fusion
   `Σ 1/(60 + rank)`, at most 4 chunks per paper, then one best chunk per unrepresented paper for
   coverage (scopes ≤ 12 papers).
3. **Evidence bundle** — up to 10 passages within a ~24k-character budget, ids `E1…`, stored per attempt
   (`retrieved_evidence`) with page and excerpt snapshots.
4. **Generation** — versioned prompt (`answering/prompt.rs`, `PROMPT_VERSION`); passages are wrapped as
   data and the model is told to ignore instructions inside them. Output is a JSON object
   `{answer_status, claims[{text, kind, evidence_ids, quote}], limitations}`. One bounded repair request
   if the JSON is invalid.
5. **Validation** (`domain/evidence.rs`) — claims citing ids outside the bundle are dropped; quotes must be
   found (after typographic normalization) uniquely inside the cited passage to get an exact span;
   numbers in a claim that are absent from its passages flag `unverifiedNumber`; papers deleted during
   generation cannot be cited. The result is `answered`, `partial` or `no_evidence`.
6. **Opening a source** — a citation's char span maps to the extracted PDFium text segments of that page;
   their PDF-space boxes are drawn through the pdf.js viewport (rotation-aware). Without a reliable
   span the page opens with the excerpt and a "position not located" note.

## Indexing pipeline (jobs)

`extract` → `index` → `embed`, persisted in `jobs` with checkpoints; one worker thread; pause / resume /
cancel / retry; interrupted jobs resume on start. `index` builds a new generation (page-bounded chunks,
≈600-token target) and publishes it atomically (`publish_generation`: old chunks, FTS rows and vectors
removed in the same transaction). `embed` fills vectors for the active generation and marks the paper
semantically ready only if that generation is still active. Changing the embedding model resets
semantic readiness and re-embeds; keyword search keeps working meanwhile.

## Model providers

| Provider | Auth | Generation | Embeddings |
|---|---|---|---|
| Ollama | none, loopback only (`127.0.0.1`), cloud-hosted Ollama models filtered out | `/api/chat` stream, `think:false` | `/api/embed`, `truncate:false` |
| OpenAI-compatible (OpenAI, Gemini, xAI, Mistral, DeepSeek, OpenRouter, Groq, Together, Fireworks, Cerebras, GitHub Models, Hugging Face, NVIDIA, Moonshot, Qwen, Z.ai, Cohere, LiteLLM, LM Studio…; presets in `src/features/models/catalog.ts`) | API key; keyless only for a loopback `http://` gateway | `/chat/completions` SSE | `/embeddings` |
| Anthropic | API key | `/v1/messages` SSE | — |
| ChatGPT plan | Sign in with ChatGPT: OAuth + OIDC, PKCE S256, dynamic client registration, loopback redirect, ID token verified against OpenAI JWKS, `chatgpt.tokens.use.direct` scope required; tokens refreshed before expiry | Responses API, `store:false`, `stream:true`, `instructions` | — |
| GitHub Copilot | OAuth Device Flow with the app's GitHub OAuth App; official `github-copilot-sdk` (bundled CLI), per-account `COPILOT_HOME`, no tools, deny-all permissions | SDK session | — |

Secrets live in Windows Credential Manager (`keyring`); SQLite stores only a reference. Cloud
providers receive paper text only after the user ticks the per-provider consent.

The GitHub OAuth App client id is `bootstrap::GITHUB_OAUTH_CLIENT_ID` (override with the
`PAPRYVIO_GITHUB_CLIENT_ID` environment variable at build time). The default id is the one already
registered for Rafiq, so GitHub's consent page shows that app's name until a Papryvio app is
registered.

## Persistence

`migrations/0001_init.sql` (library, pages, spans, jobs, settings, onboarding) and
`0002_models_index_answers.sql` (providers, generations, chunks + external-content FTS5 with triggers,
embeddings, conversations, attempts, evidence, citations, comparisons). WAL, foreign keys, busy timeout;
one connection behind a mutex, all access on blocking threads.

## Generation runner

Answers and comparison cells run on their own worker (`application/generation`), one model request at
a time, each with a cancellation token, so a question never waits behind indexing.
