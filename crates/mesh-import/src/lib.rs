mod error;
mod scene;

pub use error::ImportError;
pub use scene::{Material, Texture, Triangle, TriangleScene, load_scene};
