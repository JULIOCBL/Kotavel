//! Built-in message catalog and strict named-parameter interpolation.

mod catalog;
mod error;
mod messages;

pub use error::MessageError;
pub use messages::{MessageParameter, Messages};
