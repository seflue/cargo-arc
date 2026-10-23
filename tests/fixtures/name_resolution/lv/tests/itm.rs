mod gm;
mod tother;
mod tuser;

#[test]
fn t() {
    let _ = gm::G;
    tuser::go(lv::gm::G);
}
