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
    /// Output directly to the output directory without creating a subdirectory named after the book
    #[clap(short, long)]
    flat: bool,
}

fn main() -> Result<(), Error> {
    let args = Args::parse();
    convert_epub_to_mdbook(args.input_epub, args.output_dir, !args.flat)?;
    println!("Conversion completed successfully!");
    Ok(())
}
