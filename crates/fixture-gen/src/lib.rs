mod block;
mod bowl;
mod dome;
mod globe;
mod gyroscope;
mod pixel;
mod tent;

use std::{error::Error, path::Path};

pub use block::block_model;
pub use bowl::bowl_model;
pub use dome::dome_model;
pub use globe::globe_model;
pub use gyroscope::gyroscope_model;
pub use tent::tent_model;

use crate::pixel::save_package;

pub fn generate_examples(output: &Path) -> Result<(), Box<dyn Error>> {
    std::fs::create_dir_all(output)?;
    save_package(output, "block.depthsprite", &block_model()?)?;
    save_package(output, "bowl.depthsprite", &bowl_model()?)?;
    save_package(output, "globe.depthsprite", &globe_model()?)?;
    save_package(output, "gyroscope.depthsprite", &gyroscope_model()?)?;
    save_package(output, "tent.depthsprite", &tent_model()?)?;
    save_package(output, "dome.depthsprite", &dome_model()?)?;
    Ok(())
}
