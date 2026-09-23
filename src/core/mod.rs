// core/mod.rs - Public API surface of the core logic layer.

pub mod bookmarks;
pub mod clipboard;
pub mod entry;
pub mod listing;
pub mod recents;
pub mod transfer;

pub use clipboard::{Clipboard, ClipMode};
pub use entry::Entry;
pub use listing::{read_dir, ListError, Listing};
pub use recents::Recents;
pub use transfer::{Job, TransferClient, TransferError};
