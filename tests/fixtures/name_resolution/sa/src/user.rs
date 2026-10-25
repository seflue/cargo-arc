use crate::{self as named};

#[cfg(test)]
mod tests {
    use super::*;
    use named::conf::Settings;

    #[test]
    fn reads_the_settings() {
        let _ = Settings;
    }
}
