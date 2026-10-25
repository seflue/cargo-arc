use segs::*;

mod inner {
    use super::*;

    #[test]
    fn takes_a_name_through_the_parent() {
        other();
    }
}
