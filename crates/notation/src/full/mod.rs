//! The full multipart score's data model (parts, staves, quantised bars),
//! and its multi-staff system layout.

pub(crate) mod layout;
mod model;

pub use model::{
    BarCol, Cell, Clef, Ev, FullScore, Group, Head, NoteChord, Notehead, PartBar, PartId, PartScore, StaffDef,
};
