//! Song vocabulary shared by every engine crate: the typed song model, pitch classes,
//! chords, phonemes and G2P, the loose-JSON boundary, the reply schema and note events.

pub mod chord;
pub mod events;
pub mod g2p;
pub mod model;
pub mod phoneme;
pub mod pitch;
pub mod schema;
pub mod wire;
