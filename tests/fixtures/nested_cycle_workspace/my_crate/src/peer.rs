use crate::outer::inner::{InnerKind, InnerThing};

pub struct PeerThing;

pub fn wrap(_: InnerThing, _: InnerKind) -> PeerThing {
    PeerThing
}
