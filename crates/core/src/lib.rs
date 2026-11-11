//! Plain project data, exact timeline arithmetic, reversible edits and pure evaluation.
//! No decoder, filesystem, GPU or UI resources belong in this crate.
mod command;
mod keyframes;
mod model;
mod time;

pub use command::*;
pub use keyframes::*;
pub use model::*;
pub use time::*;

/// Invalid data, an unavailable edit target or arithmetic outside the timeline domain.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct Error(pub String);

pub(crate) fn invalid(message: &str) -> Error {
    Error(message.to_owned())
}
