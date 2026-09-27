use crate::outer::inner::InnerThing;

pub struct PeerThing;

pub fn wrap(_: InnerThing) -> PeerThing {
    PeerThing
}
