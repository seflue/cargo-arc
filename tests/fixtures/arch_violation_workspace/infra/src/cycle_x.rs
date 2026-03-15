use crate::cycle_y;

pub fn x_value() -> String {
    format!("x + {}", cycle_y::y_helper())
}

pub fn x_helper() -> &'static str {
    "x_help"
}
