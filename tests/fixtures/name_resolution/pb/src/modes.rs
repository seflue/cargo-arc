mod kid;

use self::Shape::Round;

pub enum Shape {
    Round,
}

pub(crate) fn run() {
    kid::k();
}
