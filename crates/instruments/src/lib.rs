//! Sound models: plucked string, guitar with sympathetic strings, body impulse responses,
//! bowed string and drum voices. Each renders note events from `song::events` into a buffer.

pub mod body;
pub mod drums;
pub mod guitar;
pub mod pluck;
pub mod violin;
