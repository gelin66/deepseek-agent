//! Minimal ordered context blocks consumed by the production prompt builder.

mod fragment;
mod world_state;

pub use fragment::{FragmentId, FragmentRole, ModelContextFragment};
pub use world_state::{WorldState, WorldStateSnapshot};
