use std::{sync::{LazyLock, Mutex}, time::{Duration, Instant}};

use ahash::HashMap;


pub struct DebugState {
    timings: HashMap<String, Duration>,
    timing_changes: HashMap<String, Instant>,
}

impl DebugState {
    pub fn new() -> Self {
        Self {
            timings: HashMap::default(),
            timing_changes: HashMap::default(),
        }
    }

    pub fn add_timing(&mut self, name: &str, duration: Duration) {
        self.timings.insert(name.to_string(), duration);
        self.timing_changes.insert(name.to_string(), Instant::now());
    }

    pub fn iter_timings(&self) -> impl Iterator<Item = (&String, &Duration)> {
        self.timings.iter()
    }

    pub fn timing_changed(&self, name: &str) -> f32 {
        self.timing_changes.get(name).map(|instant| {
            let duration = instant.elapsed();
            duration.as_millis() as f32 / 1000.0
        }).unwrap_or(0.0)
    }
}

pub static DEBUG_STATE: LazyLock<Mutex<DebugState>> = LazyLock::new(|| Mutex::new(DebugState::new()));