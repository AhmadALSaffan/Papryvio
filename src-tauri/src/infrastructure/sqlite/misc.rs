//! Settings, onboarding progress and idempotency keys.
use rusqlite::{params, OptionalExtension};

use crate::application::error::AppResult;
use crate::application::ports::{IdempotencyStore, OnboardingRepository, SettingsRepository, StepRecord};
use crate::domain::onboarding::{StepKey, StepState};

use super::SqliteStore;

impl SettingsRepository for SqliteStore {
    fn get(&self, key: &str) -> AppResult<Option<String>> {
        self.db.with(|c| {
            c.query_row("SELECT value FROM settings WHERE key = ?1", params![key], |r| r.get(0)).optional()
        })
    }

    fn set(&self, key: &str, value: &str, now: i64) -> AppResult<()> {
        self.db
            .with(|c| {
                c.execute(
                    "INSERT INTO settings (key, value, updated_at) VALUES (?1, ?2, ?3)
                     ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
                    params![key, value, now],
                )
            })
            .map(|_| ())
    }
}

impl OnboardingRepository for SqliteStore {
    fn list(&self, flow_version: i64) -> AppResult<Vec<StepRecord>> {
        self.db.with(|c| {
            let mut st = c.prepare(
                "SELECT step_key, state, related_entity_id, updated_at FROM onboarding_progress WHERE flow_version = ?1",
            )?;
            let rows = st.query_map(params![flow_version], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<String>>(2)?, r.get::<_, i64>(3)?))
            })?;
            let mut out = Vec::new();
            for row in rows {
                let (k, s, rel, at) = row?;
                if let (Ok(key), Ok(state)) = (StepKey::parse(&k), StepState::parse(&s)) {
                    out.push(StepRecord { key, state, related_entity_id: rel, updated_at: at });
                }
            }
            Ok(out)
        })
    }

    fn set(&self, flow_version: i64, step: StepKey, state: StepState, related: Option<&str>, now: i64) -> AppResult<()> {
        self.db
            .with(|c| {
                c.execute(
                    "INSERT INTO onboarding_progress (flow_version, step_key, state, related_entity_id, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5)
                     ON CONFLICT(flow_version, step_key) DO UPDATE SET
                        state = excluded.state, related_entity_id = excluded.related_entity_id, updated_at = excluded.updated_at",
                    params![flow_version, step.as_str(), state.as_str(), related, now],
                )
            })
            .map(|_| ())
    }

    fn reset(&self, flow_version: i64) -> AppResult<()> {
        self.db
            .with(|c| c.execute("DELETE FROM onboarding_progress WHERE flow_version = ?1", params![flow_version]))
            .map(|_| ())
    }
}

impl IdempotencyStore for SqliteStore {
    fn get(&self, request_id: &str) -> AppResult<Option<(String, String, String)>> {
        self.db.with(|c| {
            c.query_row(
                "SELECT command, payload_hash, response FROM idempotency_keys WHERE request_id = ?1",
                params![request_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
        })
    }

    fn put(&self, request_id: &str, command: &str, payload_hash: &str, response: &str, now: i64) -> AppResult<()> {
        self.db
            .with(|c| {
                c.execute(
                    "INSERT OR REPLACE INTO idempotency_keys (request_id, command, payload_hash, response, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![request_id, command, payload_hash, response, now],
                )
            })
            .map(|_| ())
    }

    fn prune_older_than(&self, cutoff: i64) -> AppResult<()> {
        self.db
            .with(|c| c.execute("DELETE FROM idempotency_keys WHERE created_at < ?1", params![cutoff]))
            .map(|_| ())
    }
}
