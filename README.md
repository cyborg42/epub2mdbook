# EPUB to MDBook Converter

[![Crates.io](https://img.shields.io/crates/v/epub2mdbook.svg)](https://crates.io/crates/epub2mdbook)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

A fast and reliable tool to convert EPUB e-books to [MDBook](https://github.com/rust-lang/mdBook) format.

## Features

- 📖 Converts EPUB content (XHTML/HTML) to Markdown
- 📑 Automatically generates `SUMMARY.md` from the EPUB table of contents
- 📝 Creates `book.toml` with metadata (title, authors, description, language)
- 🖼️ Preserves images and other resources
- 🔗 Fixes internal links to point to converted Markdown files

## Installation

### From crates.io

```bash
cargo install epub2mdbook
```

### From source

```bash
git clone https://github.com/cyborg42/epub2mdbook.git
cd epub2mdbook
cargo install --path .
```

## Usage

### Command Line

```bash
# Basic usage - creates a subdirectory named after the book
epub2mdbook book.epub

# Specify output directory
epub2mdbook book.epub --output-dir ./output

# Output directly to the directory without creating a subdirectory
epub2mdbook book.epub --output-dir ./my-book --flat
```

### As a Library

Add to your `Cargo.toml`:

```toml
[dependencies]
epub2mdbook = "0.16"
```

Then use in your code:

```rust
use epub2mdbook::convert_epub_to_mdbook;

fn main() -> Result<(), epub2mdbook::error::Error> {
    // Creates ./output/book_name/ with the converted content
    convert_epub_to_mdbook("book.epub", "./output", true)?;

    // Or output directly to ./my-book/ without subdirectory
    convert_epub_to_mdbook("book.epub", "./my-book", false)?;

    Ok(())
}
```

## Output Structure

```
output/
└── book_name/
    ├── book.toml
    └── src/
        ├── SUMMARY.md
        ├── chapter1.md
        ├── chapter2.md
        └── images/
            └── cover.png
```

## License

This project is licensed under the MIT License
