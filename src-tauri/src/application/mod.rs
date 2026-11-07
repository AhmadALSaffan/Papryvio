//! Application layer: use cases orchestrating domain rules through ports.
use std::sync::Arc;

pub mod error;
pub mod ingestion;
pub mod jobs;
pub mod library;
pub mod onboarding;
pub mod ports;
pub mod settings;
pub mod views;
pub mod views2;
pub mod models;
pub mod indexing;
pub mod retrieval;
pub mod answering;
pub mod comparison;
pub mod generation;

use ports::{
    AnswerRepository, Clock, CollectionRepository, DocumentRepository, EventSink, FileStore, IdempotencyStore,
    IndexRepository, JobRepository, OnboardingRepository, PageRepository, PdfExtractor, ProviderFactory,
    ProviderRepository, SecretStore, SettingsRepository, SignInService,
};

/// The set of adapters a use case may need. Built once in `bootstrap.rs`.
pub struct Services {
    pub clock: Arc<dyn Clock>,
    pub collections: Arc<dyn CollectionRepository>,
    pub documents: Arc<dyn DocumentRepository>,
    pub pages: Arc<dyn PageRepository>,
    pub jobs: Arc<dyn JobRepository>,
    pub settings: Arc<dyn SettingsRepository>,
    pub onboarding: Arc<dyn OnboardingRepository>,
    pub idempotency: Arc<dyn IdempotencyStore>,
    pub files: Arc<dyn FileStore>,
    pub pdf: Arc<dyn PdfExtractor>,
    pub events: Arc<dyn EventSink>,
    pub index: Arc<dyn IndexRepository>,
    pub answers: Arc<dyn AnswerRepository>,
    pub providers: Arc<dyn ProviderRepository>,
    pub secrets: Arc<dyn SecretStore>,
    pub provider_factory: Arc<dyn ProviderFactory>,
    pub sign_in: Arc<dyn SignInService>,
}

impl Services {
    pub fn now(&self) -> i64 {
        self.clock.now_ms()
    }
}

/// SHA-256 hex of a payload, used to detect a reused request id with different input.
pub fn payload_hash(payload: &str) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(payload.as_bytes()))
}
