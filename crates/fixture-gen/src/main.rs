use std::{
    error::Error,
    ffi::OsString,
    path::{Path, PathBuf},
};

use fixture_gen::generate_examples;

fn main() -> Result<(), Box<dyn Error>> {
    let output = output_directory(std::env::args_os())?;
    generate_examples(&output)?;
    Ok(())
}

fn output_directory(mut arguments: impl Iterator<Item = OsString>) -> Result<PathBuf, String> {
    let program = arguments
        .next()
        .unwrap_or_else(|| OsString::from("fixture-gen"));
    let Some(output) = arguments.next() else {
        return Err(format!(
            "usage: {} <output-directory>",
            Path::new(&program).display()
        ));
    };
    if arguments.next().is_some() {
        return Err(format!(
            "usage: {} <output-directory>",
            Path::new(&program).display()
        ));
    }
    Ok(PathBuf::from(output))
}
