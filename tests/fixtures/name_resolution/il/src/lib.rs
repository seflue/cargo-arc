mod deep_user;
mod holder;
mod own;
mod scopes;
mod user;

pub(crate) mod inner {
    pub use crate::scopes::*;
    pub struct Local;
    pub mod deep {
        pub struct Deep;
    }
}

pub use crate::inner::ScopeId;
pub use holder::inl;

macro_rules! make {
    ($name:ident) => {
        pub struct $name;
    };
}
make!(Made);
