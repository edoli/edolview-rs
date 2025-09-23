pub trait Join {
    fn join(&self, sep: &str) -> String;
}

impl<T: std::fmt::Display> Join for Vec<T> {
    fn join(&self, sep: &str) -> String {
        let strings = self.iter().map(|n| n.to_string()).collect::<Vec<_>>();
        <[String]>::join(&strings, sep)
    }
}
