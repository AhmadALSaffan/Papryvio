//! Connected providers, model choices, cloud consent and real model checks.
use std::sync::Arc;
use std::time::Instant;

use serde::de::DeserializeOwned;

use crate::application::error::{AppError, AppResult, ErrorCode};
use crate::application::jobs::{JobControl, JobScheduler};
use crate::application::ports::{AppEvent, ChatRequest, ModelInfo, ModelProvider, NewJob, ProviderKind, ProviderRecord, SignInPoll};
use crate::application::views::{EmbeddingChoice, ModelChoice, ModelSettingsView, ModelTestResult, ProviderView};
use crate::application::Services;
use crate::domain::ids::{AttemptId, JobId};
use crate::domain::job::JobKind;
use crate::domain::onboarding::{StepKey, StepState, FLOW_VERSION};

const GENERATION_KEY: &str = "model.generation";
const EMBEDDING_KEY: &str = "model.embedding";
const CONSENT_KEY: &str = "model.cloud_consent";

fn read_json<T: DeserializeOwned>(svc: &Services, key: &str) -> AppResult<Option<T>> {
    Ok(svc.settings.get(key)?.filter(|v| !v.is_empty()).and_then(|v| serde_json::from_str(&v).ok()))
}

pub fn generation_choice(svc: &Services) -> AppResult<Option<ModelChoice>> {
    read_json(svc, GENERATION_KEY)
}

pub fn embedding_choice(svc: &Services) -> AppResult<Option<EmbeddingChoice>> {
    read_json(svc, EMBEDDING_KEY)
}

pub fn consented(svc: &Services) -> AppResult<Vec<String>> {
    Ok(read_json::<Vec<String>>(svc, CONSENT_KEY)?.unwrap_or_default())
}

pub fn embedding_fingerprint(record: &ProviderRecord, model: &str, digest: Option<&str>, dims: u32) -> String {
    format!("{}|{}|{}|{}|{}|l2|task=none|v1", record.kind.as_str(), record.base_url, model, digest.unwrap_or("-"), dims)
}

fn provider_view(svc: &Services, p: &ProviderRecord) -> ProviderView {
    let has_secret = p.secret_ref.as_deref().is_some_and(|r| svc.secrets.get(r).ok().flatten().is_some());
    ProviderView {
        id: p.id.clone(),
        kind: p.kind,
        label: p.label.clone(),
        base_url: p.base_url.clone(),
        is_local: p.kind.is_local(),
        has_secret,
        account: p.external_id.clone().filter(|_| matches!(p.kind, ProviderKind::Chatgpt | ProviderKind::GithubCopilot)),
    }
}

pub fn model_settings(svc: &Services) -> AppResult<ModelSettingsView> {
    let providers = svc.providers.list()?;
    let embedding = embedding_choice(svc)?;
    let indexed = svc.index.indexed_documents()?;
    let semantic = match &embedding {
        Some(e) => svc
            .index
            .searchable_docs(&indexed)?
            .iter()
            .filter(|d| d.semantic_fingerprint.as_deref() == Some(e.fingerprint.as_str()))
            .count() as u32,
        None => 0,
    };
    Ok(ModelSettingsView {
        providers: providers.iter().map(|p| provider_view(svc, p)).collect(),
        generation: generation_choice(svc)?,
        embedding,
        cloud_consent: consented(svc)?,
        indexed_docs: indexed.len() as u32,
        semantic_docs: semantic,
    })
}

fn provider(svc: &Services, id: &str) -> AppResult<ProviderRecord> {
    svc.providers.get(id)?.ok_or_else(|| AppError::not_found("provider"))
}

pub fn open(svc: &Services, id: &str) -> AppResult<(ProviderRecord, Arc<dyn ModelProvider>)> {
    let record = provider(svc, id)?;
    let client = svc.provider_factory.open(&record)?;
    Ok((record, client))
}

/// Only loopback addresses are accepted for the local Ollama provider.
pub fn validate_loopback(url: &str) -> AppResult<String> {
    let u = url.trim().trim_end_matches('/');
    let rest = u.strip_prefix("http://").ok_or_else(|| AppError::validation("Ollama address must start with http://"))?;
    let host = rest.split('/').next().unwrap_or("");
    let host_only = host.rsplit_once(':').map(|(h, _)| h).unwrap_or(host);
    if !matches!(host_only, "127.0.0.1" | "localhost" | "[::1]") {
        return Err(AppError::validation("the local provider must use a loopback address"));
    }
    if rest.contains('/') {
        return Err(AppError::validation("use the server address only, without a path"));
    }
    Ok(u.replace("localhost", "127.0.0.1"))
}

fn validate_remote(url: &str) -> AppResult<String> {
    let u = url.trim().trim_end_matches('/').to_string();
    if u.starts_with("https://") {
        return Ok(u);
    }
    if u.starts_with("http://") && validate_loopback(u.split("/v1").next().unwrap_or(&u)).is_ok() {
        return Ok(u); // a local OpenAI-compatible server (LM Studio, llama.cpp)
    }
    Err(AppError::validation("API address must use https (or a loopback http address)"))
}

pub fn add_ollama(svc: &Services, base_url: &str) -> AppResult<ProviderView> {
    let base_url = validate_loopback(base_url)?;
    let now = svc.now();
    let record = ProviderRecord {
        id: uuid::Uuid::now_v7().to_string(),
        kind: ProviderKind::Ollama,
        label: "Ollama".into(),
        base_url,
        secret_ref: None,
        external_id: None,
        config: serde_json::json!({}),
        created_at: now,
    };
    svc.providers.save(&record, now)?;
    svc.events.emit(AppEvent::ModelsChanged);
    Ok(provider_view(svc, &record))
}

pub fn add_api_provider(svc: &Services, kind: ProviderKind, label: &str, base_url: &str, api_key: &str) -> AppResult<ProviderView> {
    if !matches!(kind, ProviderKind::OpenaiCompatible | ProviderKind::Anthropic) {
        return Err(AppError::validation("this provider is connected by signing in"));
    }
    let key = api_key.trim();
    let base_url = validate_remote(base_url)?;
    // A local OpenAI-compatible gateway (LiteLLM, LM Studio) may run without a key; remote APIs never do.
    let keyless = key.is_empty() && kind == ProviderKind::OpenaiCompatible && base_url.starts_with("http://");
    if !keyless && (key.len() < 8 || key.len() > 512 || key.chars().any(|c| c.is_whitespace() || c.is_control())) {
        return Err(AppError::validation("invalid API key"));
    }
    let label = label.trim();
    if label.is_empty() || label.chars().count() > 60 {
        return Err(AppError::validation("invalid label"));
    }
    let now = svc.now();
    let id = uuid::Uuid::now_v7().to_string();
    let secret_ref = if keyless {
        None
    } else {
        let r = format!("provider:{id}");
        svc.secrets.set(&r, &serde_json::json!({ "api_key": key }).to_string())?;
        Some(r)
    };
    let record = ProviderRecord {
        id,
        kind,
        label: label.to_string(),
        base_url,
        secret_ref,
        external_id: None,
        config: serde_json::json!({}),
        created_at: now,
    };
    if let Err(e) = svc.providers.save(&record, now) {
        if let Some(r) = record.secret_ref.as_deref() {
            let _ = svc.secrets.delete(r);
        }
        return Err(e);
    }
    svc.events.emit(AppEvent::ModelsChanged);
    Ok(provider_view(svc, &record))
}

/// Polls a sign-in flow; on success stores the tokens in the OS credential store.
pub fn poll_sign_in(svc: &Services, kind: ProviderKind, flow_id: &str) -> AppResult<(String, Option<ProviderView>)> {
    match svc.sign_in.poll(flow_id)? {
        SignInPoll::Pending => Ok(("pending".into(), None)),
        SignInPoll::Denied(m) => Ok((format!("denied:{m}"), None)),
        SignInPoll::Expired => Ok(("expired".into(), None)),
        SignInPoll::Failed(m) => Ok((format!("error:{m}"), None)),
        SignInPoll::Complete { label, external_id, secret_json } => {
            let now = svc.now();
            // Reconnecting the same account replaces its tokens instead of adding a duplicate.
            let existing = svc
                .providers
                .list()?
                .into_iter()
                .find(|p| p.kind == kind && p.external_id.as_deref() == Some(external_id.as_str()));
            let id = existing.as_ref().map(|p| p.id.clone()).unwrap_or_else(|| uuid::Uuid::now_v7().to_string());
            let secret_ref = format!("provider:{id}");
            svc.secrets.set(&secret_ref, &secret_json)?;
            let base_url = match kind {
                ProviderKind::Chatgpt => "https://api.openai.com/v1".to_string(),
                ProviderKind::GithubCopilot => "https://api.githubcopilot.com".to_string(),
                _ => return Err(AppError::validation("not a sign-in provider")),
            };
            let record = ProviderRecord {
                id,
                kind,
                label: if kind == ProviderKind::Chatgpt { "ChatGPT".into() } else { "GitHub Copilot".into() },
                base_url,
                secret_ref: Some(secret_ref),
                external_id: Some(label),
                config: serde_json::json!({ "subject": external_id }),
                created_at: existing.map(|e| e.created_at).unwrap_or(now),
            };
            svc.providers.save(&record, now)?;
            svc.events.emit(AppEvent::ModelsChanged);
            Ok(("complete".into(), Some(provider_view(svc, &record))))
        }
    }
}

pub fn remove_provider(svc: &Services, id: &str) -> AppResult<()> {
    let record = provider(svc, id)?;
    if let Some(r) = &record.secret_ref {
        svc.secrets.delete(r)?;
    }
    svc.providers.delete(id)?;
    let now = svc.now();
    if generation_choice(svc)?.is_some_and(|c| c.provider_id == id) {
        svc.settings.set(GENERATION_KEY, "", now)?;
    }
    if embedding_choice(svc)?.is_some_and(|c| c.provider_id == id) {
        svc.settings.set(EMBEDDING_KEY, "", now)?;
        svc.index.reset_semantic(None)?;
    }
    let consent: Vec<String> = consented(svc)?.into_iter().filter(|c| c != id).collect();
    svc.settings.set(CONSENT_KEY, &serde_json::to_string(&consent).unwrap(), now)?;
    svc.events.emit(AppEvent::ModelsChanged);
    Ok(())
}

pub fn list_models(svc: &Services, provider_id: &str) -> AppResult<Vec<ModelInfo>> {
    let (_, client) = open(svc, provider_id)?;
    client.list_models()
}

pub fn set_consent(svc: &Services, provider_id: &str, allowed: bool) -> AppResult<ModelSettingsView> {
    provider(svc, provider_id)?;
    let mut consent = consented(svc)?;
    consent.retain(|c| c != provider_id);
    if allowed {
        consent.push(provider_id.to_string());
    }
    svc.settings.set(CONSENT_KEY, &serde_json::to_string(&consent).unwrap(), svc.now())?;
    model_settings(svc)
}

/// Paper text may only go to a cloud provider the user explicitly allowed.
pub fn ensure_allowed(svc: &Services, record: &ProviderRecord) -> AppResult<()> {
    if record.kind.is_local() || consented(svc)?.contains(&record.id) {
        Ok(())
    } else {
        Err(AppError::new(ErrorCode::NotAllowed, "cloud provider not approved for paper text"))
    }
}

/// Real check: a short embedding (dimensions verified) or a short generation.
pub fn test_model(svc: &Services, provider_id: &str, model: &str, purpose: &str) -> AppResult<ModelTestResult> {
    let (_, client) = open(svc, provider_id)?;
    let started = Instant::now();
    let result = if purpose == "embedding" {
        client.embed(model, &["Papryvio checks that this embedding model responds.".to_string()]).and_then(|v| {
            let first = v.into_iter().next().ok_or_else(|| AppError::internal("no vector returned"))?;
            crate::domain::retrieval::normalize_vector(&first, None).map_err(AppError::validation)?;
            Ok((Some(first.len() as u32), None))
        })
    } else {
        let req = ChatRequest {
            system: "You are a connectivity check. Reply with the single word OK.".into(),
            user: "Reply with OK.".into(),
            json: false,
            max_output_tokens: 16,
        };
        let control = JobControl::new();
        client.chat(model, &req, &control, &mut |_| {}).map(|o| (None, Some(o.text.chars().take(40).collect::<String>())))
    };
    let elapsed_ms = started.elapsed().as_millis() as u64;
    Ok(match result {
        Ok((dimensions, sample)) => ModelTestResult { ok: true, dimensions, sample, elapsed_ms, error: None },
        Err(e) => ModelTestResult { ok: false, dimensions: None, sample: None, elapsed_ms, error: Some(e.detail) },
    })
}

pub fn save_generation(svc: &Services, provider_id: &str, model: &str) -> AppResult<ModelSettingsView> {
    let record = provider(svc, provider_id)?;
    ensure_allowed(svc, &record)?;
    let test = test_model(svc, provider_id, model, "generation")?;
    if !test.ok {
        return Err(AppError::new(ErrorCode::Unsupported, test.error.unwrap_or_default()).retryable());
    }
    let choice = ModelChoice { provider_id: provider_id.into(), model: model.into() };
    svc.settings.set(GENERATION_KEY, &serde_json::to_string(&choice).unwrap(), svc.now())?;
    // A generation model answered a real request: the model step of the guide is done.
    svc.onboarding.set(FLOW_VERSION, StepKey::Models, StepState::Completed, Some(provider_id), svc.now())?;
    svc.events.emit(AppEvent::ModelsChanged);
    model_settings(svc)
}

/// Saving a different embedding model invalidates every vector: documents lose semantic
/// readiness and are re-embedded with the new model (keyword search keeps working).
pub fn save_embedding(svc: &Services, scheduler: &JobScheduler, provider_id: &str, model: &str) -> AppResult<ModelSettingsView> {
    let record = provider(svc, provider_id)?;
    ensure_allowed(svc, &record)?;
    let test = test_model(svc, provider_id, model, "embedding")?;
    let dims = match (test.ok, test.dimensions) {
        (true, Some(d)) => d,
        _ => return Err(AppError::new(ErrorCode::Unsupported, test.error.unwrap_or_else(|| "no vector".into())).retryable()),
    };
    let digest = list_models(svc, provider_id).ok().and_then(|ms| ms.into_iter().find(|m| m.id == model).and_then(|m| m.digest));
    let fingerprint = embedding_fingerprint(&record, model, digest.as_deref(), dims);
    let previous = embedding_choice(svc)?;
    let choice = EmbeddingChoice { provider_id: provider_id.into(), model: model.into(), dimensions: dims, digest, fingerprint: fingerprint.clone() };
    svc.settings.set(EMBEDDING_KEY, &serde_json::to_string(&choice).unwrap(), svc.now())?;
    if previous.map(|p| p.fingerprint) != Some(fingerprint.clone()) {
        svc.index.reset_semantic(Some(&fingerprint))?;
        for doc in svc.index.indexed_documents()? {
            enqueue(svc, scheduler, JobKind::Embed, doc.as_str())?;
        }
    }
    svc.events.emit(AppEvent::ModelsChanged);
    svc.events.emit(AppEvent::LibraryChanged);
    model_settings(svc)
}

pub fn clear_embedding(svc: &Services) -> AppResult<ModelSettingsView> {
    svc.settings.set(EMBEDDING_KEY, "", svc.now())?;
    svc.index.reset_semantic(None)?;
    svc.events.emit(AppEvent::ModelsChanged);
    model_settings(svc)
}

pub(crate) fn enqueue(svc: &Services, scheduler: &JobScheduler, kind: JobKind, entity: &str) -> AppResult<JobId> {
    if let Some(existing) = svc.jobs.latest_for_entity(entity, kind)? {
        if existing.state.is_active() {
            return Ok(existing.id);
        }
    }
    let job = NewJob { id: JobId::new(), kind, entity_id: entity.into(), attempt_id: AttemptId::new(), stage: "queued".into(), now: svc.now() };
    svc.jobs.insert(&job)?;
    scheduler.enqueue(job.id.clone());
    Ok(job.id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_only_for_ollama() {
        assert_eq!(validate_loopback("http://localhost:11434/").unwrap(), "http://127.0.0.1:11434");
        assert!(validate_loopback("http://192.168.1.5:11434").is_err());
        assert!(validate_loopback("https://ollama.example.com").is_err());
        assert!(validate_loopback("http://127.0.0.1:11434/api").is_err());
        assert!(validate_remote("https://api.openai.com/v1").is_ok());
        assert!(validate_remote("http://example.com/v1").is_err());
        assert!(validate_remote("http://127.0.0.1:1234/v1").is_ok());
    }
}
