pub mod chunk;
pub mod clip;
pub mod envelope;
pub mod events;
pub mod file;
pub mod midi;
pub mod schema;
pub mod smf;
pub mod template;

pub use chunk::{item as chunk_item, read_all as read_chunks, Item, ParseResult};
pub use clip::{
    decode_clip, encode_clip, Clip, EventItem, LengthItem, NoteItem, NoteView, TopNode, Track,
    TrackChild,
};
pub use envelope::{
    decode as envelope_decode, encode as envelope_encode, Envelope, EnvelopeError, Magic,
};
pub use file::FileKind;
