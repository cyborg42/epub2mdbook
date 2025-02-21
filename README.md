# EPUB to MDBook Converter

This is a powerful tool to convert EPUB files to MDBook format.

## Usage

### CLI

```bash
cargo install epub2mdbook
epub2mdbook --input-epub path/to/input.epub --output-dir path/to/output
```

### Rust

```rust
use epub2mdbook::convert_epub_to_mdbook;

convert_epub_to_mdbook(
    PathBuf::from("path/to/input.epub"),
    Some(PathBuf::from("path/to/output")),
);
```
