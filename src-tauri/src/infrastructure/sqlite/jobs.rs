use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::application::error::AppResult;
use crate::application::ports::{JobRecord, JobRepository, NewJob};
use crate::domain::ids::{AttemptId, JobId};
use crate::domain::job::{JobKind, JobState};

use super::SqliteStore;

pub(crate) const JOB_COLUMNS: &str = "id, kind, entity_id, attempt_id, state, stage, checkpoint, completed_units, \
     total_units, error_kind, error_message, created_at, updated_at";

fn map_job(r: &Row<'_>) -> rusqlite::Result<JobRecord> {
    let kind: String = r.get(1)?;
    let state: String = r.get(4)?;
    Ok(JobRecord {
        id: JobId::from_trusted(r.get(0)?),
        kind: JobKind::parse(&kind).unwrap_or(JobKind::Extract),
        entity_id: r.get(2)?,
        attempt_id: AttemptId::from_trusted(r.get(3)?),
        state: JobState::parse(&state).unwrap_or(JobState::Failed),
        stage: r.get(5)?,
        checkpoint: r.get(6)?,
        completed_units: r.get(7)?,
        total_units: r.get(8)?,
        error_kind: r.get(9)?,
        error_message: r.get(10)?,
        created_at: r.get(11)?,
        updated_at: r.get(12)?,
    })
}

pub(crate) fn insert_job(conn: &Connection, job: &NewJob) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO jobs (id, kind, entity_id, attempt_id, state, stage, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, 'queued', ?5, ?6, ?6)",
        params![job.id.as_str(), job.kind.as_str(), job.entity_id, job.attempt_id.as_str(), job.stage, job.now],
    )?;
    Ok(())
}

pub(crate) fn job_for_entity(conn: &Connection, entity_id: &str, kind: JobKind) -> rusqlite::Result<Option<JobRecord>> {
    let mut st = conn.prepare_cached(&format!(
        "SELECT {JOB_COLUMNS} FROM jobs WHERE entity_id = ?1 AND kind = ?2 ORDER BY created_at DESC, id DESC LIMIT 1"
    ))?;
    st.query_row(params![entity_id, kind.as_str()], map_job).optional()
}

impl JobRepository for SqliteStore {
    fn insert(&self, job: &NewJob) -> AppResult<()> {
        self.db.with(|c| insert_job(c, job))
    }

    fn get(&self, id: &JobId) -> AppResult<Option<JobRecord>> {
        self.db.with(|c| {
            c.query_row(&format!("SELECT {JOB_COLUMNS} FROM jobs WHERE id = ?1"), params![id.as_str()], map_job).optional()
        })
    }

    fn latest_for_entity(&self, entity_id: &str, kind: JobKind) -> AppResult<Option<JobRecord>> {
        self.db.with(|c| job_for_entity(c, entity_id, kind))
    }

    fn save(&self, j: &JobRecord) -> AppResult<()> {
        self.db
            .with(|c| {
                c.execute(
                    "UPDATE jobs SET attempt_id = ?2, state = ?3, stage = ?4, checkpoint = ?5, completed_units = ?6,
                        total_units = ?7, error_kind = ?8, error_message = ?9, updated_at = ?10
                     WHERE id = ?1",
                    params![
                        j.id.as_str(),
                        j.attempt_id.as_str(),
                        j.state.as_str(),
                        j.stage,
                        j.checkpoint,
                        j.completed_units,
                        j.total_units,
                        j.error_kind,
                        j.error_message,
                        j.updated_at
                    ],
                )
            })
            .map(|_| ())
    }

    fn list_active(&self) -> AppResult<Vec<JobRecord>> {
        self.db.with(|c| {
            let mut st = c.prepare(&format!(
                "SELECT {JOB_COLUMNS} FROM jobs WHERE state IN ('queued','running','paused') ORDER BY created_at"
            ))?;
            let rows = st.query_map([], map_job)?;
            rows.collect()
        })
    }
}
