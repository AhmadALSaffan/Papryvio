//! Runs model-backed work (answers, comparison cells) on one worker thread, one request at
//! a time, each with its own cancellation token. Separate from the indexing queue so a
//! question never waits behind a long import.
use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};

use crate::application::jobs::JobControl;
use crate::application::Services;
use crate::application::{answering, comparison};

#[derive(Debug, Clone)]
pub enum Task {
    Answer(String),
    Comparison(String),
    Cell { comparison_id: String, cell_id: String },
}

impl Task {
    fn key(&self) -> &str {
        match self {
            Task::Answer(id) | Task::Comparison(id) => id,
            Task::Cell { cell_id, .. } => cell_id,
        }
    }
}

pub struct GenerationRunner {
    tx: Mutex<Option<Sender<Task>>>,
    controls: Mutex<HashMap<String, Arc<JobControl>>>,
}

impl GenerationRunner {
    pub fn start(svc: Arc<Services>) -> Arc<Self> {
        let (tx, rx) = mpsc::channel::<Task>();
        let runner = Arc::new(Self { tx: Mutex::new(Some(tx)), controls: Mutex::new(HashMap::new()) });
        let r = Arc::clone(&runner);
        std::thread::Builder::new()
            .name("papryvio-generation".into())
            .spawn(move || r.run(svc, rx))
            .expect("spawn generation worker");
        runner
    }

    pub fn submit(&self, task: Task) {
        let control = Arc::new(JobControl::new());
        self.controls.lock().unwrap().insert(task.key().to_string(), control);
        if let Some(tx) = self.tx.lock().unwrap().as_ref() {
            let _ = tx.send(task);
        }
    }

    /// Cancels a queued or running task. Late results of a cancelled attempt are discarded.
    pub fn cancel(&self, key: &str) -> bool {
        match self.controls.lock().unwrap().get(key) {
            Some(c) => {
                c.request_cancel();
                true
            }
            None => false,
        }
    }

    pub fn shutdown(&self) {
        for c in self.controls.lock().unwrap().values() {
            c.request_cancel();
        }
        self.tx.lock().unwrap().take();
    }

    fn run(self: Arc<Self>, svc: Arc<Services>, rx: Receiver<Task>) {
        while let Ok(task) = rx.recv() {
            let key = task.key().to_string();
            let control = self.controls.lock().unwrap().get(&key).cloned().unwrap_or_default();
            let result = match &task {
                Task::Answer(id) => answering::run_attempt(&svc, id, &control),
                Task::Comparison(id) => comparison::run_comparison(&svc, id, &control, &self),
                Task::Cell { comparison_id, cell_id } => comparison::run_single_cell(&svc, comparison_id, cell_id, &control),
            };
            if let Err(e) = result {
                tracing::error!(task = ?task, "generation task failed: {e}");
            }
            self.controls.lock().unwrap().remove(&key);
        }
    }

    /// Lets a comparison check whether one of its cells was cancelled individually.
    pub fn control(&self, key: &str) -> Option<Arc<JobControl>> {
        self.controls.lock().unwrap().get(key).cloned()
    }
}
