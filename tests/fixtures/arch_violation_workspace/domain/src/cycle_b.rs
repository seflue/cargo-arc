use crate::cycle_a;

pub fn b_value() -> String {
    format!("b + {}", cycle_a::a_helper())
}

pub fn b_helper() -> &'static str {
    "b_help"
}
