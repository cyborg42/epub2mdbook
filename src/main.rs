use std::path::PathBuf;

use clap::Parser;
use epub2mdbook::{convert_epub_to_mdbook, error::Error};

#[derive(Parser)]
struct Args {
    /// The path to the input EPUB file
    #[clap(short, long)]
    input_epub: PathBuf,
    /// The path to the output directory, working directory by default
    #[clap(short, long)]
    output_dir: Option<PathBuf>,
}

fn main() -> Result<(), Error> {
    let args = Args::parse();
    convert_epub_to_mdbook(args.input_epub, args.output_dir)?;
    println!("Conversion completed successfully!");
    Ok(())
}