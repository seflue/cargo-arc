mod bcli;
#[path = "b/gm.rs"]
mod gm;
#[path = "b/store.rs"]
mod store;
mod lvglob;

fn main() {
    let _ = (gm::G, store::kv::K);
    lvglob::go(lv::gm::G, lv::kv::K);
}
