use std::time::{Duration, Instant};
pub struct ScopedTimer<'a> {
    name: &'a str,
    t0: Instant,
    print_on_drop: bool,
}

impl<'a> ScopedTimer<'a> {
    #[must_use]
    pub fn new(name: &'a str) -> Self {
        Self {
            name,
            t0: Instant::now(),
            print_on_drop: false,
        }
    }

    pub fn with_print_on_drop(name: &'a str) -> Self {
        Self {
            name,
            t0: Instant::now(),
            print_on_drop: true,
        }
    }

    pub fn reset(&mut self) {
        self.t0 = Instant::now();
    }

    pub fn elapsed(&self) -> Duration {
        self.t0.elapsed()
    }

    pub fn print_elapsed(&self) {
        eprintln!("[{}] took: {:.2?}", self.name, self.t0.elapsed());
    }
}

impl<'a> Drop for ScopedTimer<'a> {
    fn drop(&mut self) {
        let elapsed = self.t0.elapsed();

        if self.print_on_drop {
            eprintln!("[{}] took: {:.2?}", self.name, elapsed);
        }

        #[cfg(debug_assertions)]
        crate::debug::DEBUG_STATE
            .lock()
            .unwrap()
            .timings
            .insert(self.name.to_string(), elapsed);
    }
}
