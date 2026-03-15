use crate::cycle_b;

pub fn a_value() -> String {
    format!("a + {}", cycle_b::b_helper())
}

pub fn a_helper() -> &'static str {
    "a_help"
}
