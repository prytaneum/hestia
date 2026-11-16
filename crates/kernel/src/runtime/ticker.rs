use std::sync::{Arc, Condvar, Mutex, PoisonError};

use wasmtime::Engine;

use super::limits::TICK;

#[derive(Default)]
struct Calls {
    running: usize,
    stopped: bool,
}

#[derive(Default)]
struct Shared {
    calls: Mutex<Calls>,
    wake: Condvar,
}

/// Advances the engine's epoch every tick while a call runs, so compute-bound
/// guest code yields to its timeout; idle, the thread sleeps.
pub(super) struct Ticker {
    shared: Arc<Shared>,
}

/// Keeps the epoch advancing while held.
pub(super) struct Ticking {
    shared: Arc<Shared>,
}

impl Ticker {
    pub fn start(engine: Engine) -> std::io::Result<Self> {
        let shared = Arc::new(Shared::default());
        let thread = shared.clone();
        std::thread::Builder::new()
            .name("plugin-epoch".into())
            .spawn(move || tick(&engine, &thread))?;
        Ok(Ticker { shared })
    }

    pub fn running(&self) -> Ticking {
        self.shared.update(|calls| calls.running += 1);
        Ticking {
            shared: self.shared.clone(),
        }
    }
}

impl Drop for Ticker {
    fn drop(&mut self) {
        self.shared.update(|calls| calls.stopped = true);
    }
}

impl Drop for Ticking {
    fn drop(&mut self) {
        self.shared.update(|calls| calls.running -= 1);
    }
}

impl Shared {
    fn update(&self, change: impl FnOnce(&mut Calls)) {
        change(&mut self.calls.lock().unwrap_or_else(PoisonError::into_inner));
        self.wake.notify_one();
    }
}

fn tick(engine: &Engine, shared: &Shared) {
    loop {
        let calls = shared.calls.lock().unwrap_or_else(PoisonError::into_inner);
        let calls = shared
            .wake
            .wait_while(calls, |calls| calls.running == 0 && !calls.stopped)
            .unwrap_or_else(PoisonError::into_inner);
        if calls.stopped {
            return;
        }
        drop(calls);
        std::thread::sleep(TICK);
        engine.increment_epoch();
    }
}
