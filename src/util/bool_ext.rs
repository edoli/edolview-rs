#[macro_export]
macro_rules! switch {
    ($cond:expr => $then:expr, $else:expr $(,)?) => {{
        if $cond {
            $then
        } else {
            $else
        }
    }};
}
