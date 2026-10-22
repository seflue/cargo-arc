#[macro_export]
macro_rules! call_dollar {
    () => {
        $crate::a::f()
    };
}

#[macro_export]
macro_rules! call_plain {
    () => {
        crate::a::plain()
    };
}
