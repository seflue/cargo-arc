macro_rules! call_path {
    ($p:path) => {
        $p()
    };
}

pub fn run() -> Vec<i32> {
    let s = format!("{}", crate::a::X);
    let _ = s;
    call_path!(crate::a::g);
    vec![crate::a::make()]
}
