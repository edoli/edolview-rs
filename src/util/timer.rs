use std::time::Instant;

pub struct ScopedTimer<'a> {
    name: &'a str,
    t0: Instant,
}

impl<'a> ScopedTimer<'a> {
    #[must_use]
    pub fn new(name: &'a str) -> Self {
        Self {
            name,
            t0: Instant::now(),
        }
    }

    pub fn reset(&mut self) {
        self.t0 = Instant::now();
    }

    pub fn elapsed(&self) -> std::time::Duration {
        self.t0.elapsed()
    }

    pub fn print_elapsed(&self) {
        eprintln!("[{}] took: {:.2?}", self.name, self.t0.elapsed());
    }
}

impl<'a> Drop for ScopedTimer<'a> {
    fn drop(&mut self) {
        eprintln!("[{}] took: {:.2?}", self.name, self.t0.elapsed());
    }
}
