mod extraction;
mod record;
mod similarity;
mod store;
mod write;

pub use extraction::maybe_extract;
pub use record::{MemoryRecord, NewFact, Visibility};
pub use similarity::cosine_similarity;
pub use store::MemoryStore;
pub use write::save_fact;
