use std::path::PathBuf;

use clap::Parser;
use epub2mdbook::convert_epub_to_mdbook;

#[derive(Parser)]
struct Args {
    /// The path to the input EPUB file
    #[clap(short, long)]
    input_epub: PathBuf,
    /// The path to the output directory
    #[clap(short, long)]
    output_dir: Option<PathBuf>,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    convert_epub_to_mdbook(args.input_epub, args.output_dir)?;
    println!("Conversion completed successfully!");
    Ok(())
}
