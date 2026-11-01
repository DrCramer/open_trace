//! Fixed worker pool for scheme requests: analysis runs on Rust threads, never on the UI thread, and the number of
//! concurrent analyses is bounded (the web UI sends up to 4 in parallel).
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};

type Job = Box<dyn FnOnce() + Send + 'static>;

pub struct Pool {
    tx: Mutex<Sender<Job>>,
    pub size: usize,
}

impl Pool {
    pub fn new(size: usize, name: &str) -> Pool {
        let (tx, rx) = channel::<Job>();
        let rx = Arc::new(Mutex::new(rx));
        for i in 0..size {
            let rx = rx.clone();
            std::thread::Builder::new()
                .name(format!("{}-{}", name, i))
                .spawn(move || loop {
                    let job = match rx.lock().unwrap_or_else(|e| e.into_inner()).recv() {
                        Ok(j) => j,
                        Err(_) => return,
                    };
                    // A panic in one request must not take the worker down.
                    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job));
                })
                .expect("spawn worker");
        }
        Pool { tx: Mutex::new(tx), size }
    }

    pub fn execute(&self, job: impl FnOnce() + Send + 'static) {
        let _ = self.tx.lock().unwrap_or_else(|e| e.into_inner()).send(Box::new(job));
    }
}

/// Number of analysis workers: all cores but one for the UI, at least 2.
pub fn default_size() -> usize {
    std::thread::available_parallelism().map(|n| n.get().saturating_sub(1)).unwrap_or(2).clamp(2, 16)
}
