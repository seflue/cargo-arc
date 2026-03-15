use crate::cycle_x;

pub fn y_value() -> String {
    format!("y + {}", cycle_x::x_helper())
}

pub fn y_helper() -> &'static str {
    "y_help"
}
