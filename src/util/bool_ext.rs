pub trait BoolExt {
    fn switch<T>(self, val_true: T, val_false: T) -> T;
}

impl BoolExt for bool {
    fn switch<T>(self, val_true: T, val_false: T) -> T {
        if self {
            val_true
        } else {
            val_false
        }
    }
}
