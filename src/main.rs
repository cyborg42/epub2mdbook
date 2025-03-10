use std::path::PathBuf;

use clap::Parser;
use epub2mdbook::{convert_epub_to_mdbook, error::Error};

#[derive(Parser)]
struct Args {
    /// The path to the input EPUB file
    input_epub: PathBuf,
    /// The path to the output directory
    #[clap(short, long, default_value = ".")]
    output_dir: PathBuf,
}

fn main() -> Result<(), Error> {
    let args = Args::parse();
    convert_epub_to_mdbook(args.input_epub, args.output_dir, true)?;
    println!("Conversion completed successfully!");
    Ok(())
}
