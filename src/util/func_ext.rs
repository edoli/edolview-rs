pub trait FuncExt<T> {
    fn call<F: FnOnce(&T)>(&self, f: F);
    fn cond_map<F: FnOnce(T) -> T>(self, cond: bool, f: F) -> Self;
}

impl<T> FuncExt<T> for T {
    #[inline]
    fn call<F: FnOnce(&T)>(&self, f: F) {
        f(self);
    }

    #[inline]
    fn cond_map<F: FnOnce(T) -> T>(self, cond: bool, f: F) -> Self {
        if cond {
            f(self)
        } else {
            self
        }
    }
}
