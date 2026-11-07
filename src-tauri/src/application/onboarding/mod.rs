//! Onboarding progress. Steps complete only when the real action behind them happened.
use crate::application::error::{AppError, AppResult};
use crate::application::settings::ONBOARDING_DISMISSED;
use crate::application::views::{DocumentQuery, OnboardingStepView, OnboardingView};
use crate::application::Services;
use crate::domain::document::ExtractionState;
use crate::domain::ids::{CollectionId, DocumentId};
use crate::domain::onboarding::{StepAction, StepKey, StepState, FLOW_VERSION};

pub fn get_progress(svc: &Services) -> AppResult<OnboardingView> {
    let stored = svc.onboarding.list(FLOW_VERSION)?;
    let steps = StepKey::ALL
        .into_iter()
        .map(|key| match stored.iter().find(|r| r.key == key) {
            Some(r) => OnboardingStepView { key, state: r.state, related_entity_id: r.related_entity_id.clone(), updated_at: Some(r.updated_at) },
            None => OnboardingStepView { key, state: StepState::Pending, related_entity_id: None, updated_at: None },
        })
        .collect();
    let dismissed = svc.settings.get(ONBOARDING_DISMISSED)?.as_deref() == Some("true");
    Ok(OnboardingView { flow_version: FLOW_VERSION, steps, dismissed })
}

pub fn update_step(svc: &Services, step: StepKey, action: StepAction, related: Option<&str>) -> AppResult<OnboardingView> {
    let now = svc.now();
    match action {
        StepAction::Reset => svc.onboarding.set(FLOW_VERSION, step, StepState::Pending, None, now)?,
        StepAction::Skip => svc.onboarding.set(FLOW_VERSION, step, StepState::Skipped, None, now)?,
        StepAction::Complete => {
            let related = verify_completion(svc, step, related)?;
            svc.onboarding.set(FLOW_VERSION, step, StepState::Completed, related.as_deref(), now)?;
        }
    }
    get_progress(svc)
}

/// Checks the real data behind a step before recording it as completed.
fn verify_completion(svc: &Services, step: StepKey, related: Option<&str>) -> AppResult<Option<String>> {
    match step {
        StepKey::Welcome => Ok(None),
        StepKey::Collection => {
            let id = related.and_then(CollectionId::parse).ok_or_else(|| AppError::validation("collection id required"))?;
            if !svc.collections.exists(&id)? {
                return Err(AppError::not_found("collection"));
            }
            Ok(Some(id.to_string()))
        }
        StepKey::Import => {
            let id = related.and_then(DocumentId::parse).ok_or_else(|| AppError::validation("document id required"))?;
            let doc = svc.documents.get(&id)?.ok_or_else(|| AppError::not_found("document"))?;
            if doc.deleted_at.is_some() {
                return Err(AppError::not_found("document"));
            }
            Ok(Some(id.to_string()))
        }
        StepKey::Readiness => {
            let page = svc.documents.list_views(&DocumentQuery { limit: Some(500), ..Default::default() })?;
            let ready = page.items.iter().find(|d| matches!(d.state, ExtractionState::TextReady | ExtractionState::Ready));
            match ready {
                Some(d) => Ok(Some(d.id.to_string())),
                None => Err(AppError::not_allowed("no paper has extracted text yet")),
            }
        }
        // Model setup, answering, citations and comparison verify against features
        // that record real events (model checks, generated answers, opened citations).
        // These complete automatically when the real event happens (a model answered a test,
        // an answer was generated, a citation was opened, a comparison finished).
        StepKey::Models | StepKey::FirstQuestion | StepKey::OpenSource | StepKey::Compare => {
            let done = svc.onboarding.list(FLOW_VERSION)?.into_iter().any(|r| r.key == step && r.state == StepState::Completed);
            if done {
                Ok(related.map(str::to_string))
            } else {
                Err(AppError::not_allowed("this step completes when the real action happens"))
            }
        }
    }
}

pub fn set_dismissed(svc: &Services, dismissed: bool) -> AppResult<OnboardingView> {
    svc.settings.set(ONBOARDING_DISMISSED, if dismissed { "true" } else { "false" }, svc.now())?;
    get_progress(svc)
}

/// Restarts the guided tour from the beginning (Settings → Help).
pub fn restart(svc: &Services) -> AppResult<OnboardingView> {
    svc.onboarding.reset(FLOW_VERSION)?;
    set_dismissed(svc, false)
}
