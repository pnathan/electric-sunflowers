//! The full multipart score's data model (parts, staves, quantised bars).
//! See `model` for the algorithms; this module has no drawing code (wave 1
//! draws it, in `notation::full::layout`, not yet built).

mod model;

pub use model::{
    BarCol, Cell, Clef, Ev, FullScore, Group, Head, NoteChord, Notehead, PartBar, PartId, PartScore, StaffDef,
};
