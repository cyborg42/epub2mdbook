pub mod error;

use epub::doc::{EpubDoc, NavPoint};
use error::Error;
use regex::{Captures, Regex};
use std::collections::HashMap;
use std::ffi::OsStr;
use std::io::{Read, Seek};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::{fs, io};

/// Convert an EPUB file to an MDBook
///
/// # Arguments
///
/// * `epub_path` - The path to the EPUB file
/// * `output_dir` - The path to the output directory, working directory by default
/// * `with_file_name` - Whether to use the file name as the output directory
///
pub fn convert_epub_to_mdbook(
    epub_path: impl AsRef<Path>,
    output_dir: Option<impl AsRef<Path>>,
    with_file_name: bool,
) -> Result<(), Error> {
    let epub_path = epub_path.as_ref();
    if !epub_path.is_file() {
        return Err(Error::NotAFile(epub_path.display().to_string()));
    }
    let book_name = epub_path
        .with_extension("")
        .file_name()
        .expect("unreachable")
        .to_string_lossy()
        .to_string();
    let mut output_dir = match output_dir {
        Some(output_dir) => output_dir.as_ref().to_owned(),
        None => PathBuf::from("."),
    };
    if with_file_name {
        output_dir.push(&book_name);
    }
    fs::create_dir_all(output_dir.join("src"))?;

    let mut epub_doc = EpubDoc::new(epub_path)?;
    let title = epub_doc
        .metadata
        .get("title")
        .and_then(|v| v.first().cloned())
        .unwrap_or(book_name);
    let creator = epub_doc
        .metadata
        .get("creator")
        .and_then(|v| v.first().cloned());
    let (summary_md, html_to_md) = generate_summary_md(&epub_doc, &title);
    extract_chapters_and_resources(&mut epub_doc, &output_dir, &html_to_md)?;
    fs::write(output_dir.join("src/SUMMARY.md"), summary_md)?;
    write_book_toml(&output_dir, &title, creator)?;
    Ok(())
}

fn epub_nav_to_md(
    nav: &NavPoint,
    indent: usize,
    html_to_md: &HashMap<PathBuf, PathBuf>,
) -> Option<String> {
    let file = html_to_md.get(&nav.content)?;
    let mut md = format!(
        "{}- [{}]({})\n",
        "  ".repeat(indent),
        nav.label,
        file.to_string_lossy()
    );
    for child in &nav.children {
        if let Some(child_md) = epub_nav_to_md(child, indent + 1, html_to_md) {
            md.push_str(&child_md);
        }
    }
    Some(md)
}

/// generate SUMMARY.md and the file mapping from html to md
///
/// # Arguments
///
/// * `epub_doc` - The EPUB document
/// * `title` - The title of the book
///
/// # Returns
///
/// * `summary_md` - The SUMMARY.md content
/// * `html_to_md` - The file mapping from html to md
pub fn generate_summary_md<R: Read + Seek>(
    epub_doc: &EpubDoc<R>,
    title: &str,
) -> (String, HashMap<PathBuf, PathBuf>) {
    let mut summary_md = format!("# {}\n\n", title);
    let html_to_md = epub_doc
        .resources
        .iter()
        .filter(|(_, (_, mime))| ["application/xhtml+xml", "text/html"].contains(&&**mime))
        .map(|(_, (path, _))| (path.clone(), path.with_extension("md")))
        .collect::<HashMap<PathBuf, PathBuf>>();
    for nav in &epub_doc.toc {
        if let Some(md) = epub_nav_to_md(nav, 0, &html_to_md) {
            summary_md.push_str(&md);
        }
    }
    (summary_md, html_to_md)
}

fn extract_chapters_and_resources<R: Read + Seek>(
    epub_doc: &mut EpubDoc<R>,
    output_dir: impl AsRef<Path>,
    html_to_md: &HashMap<PathBuf, PathBuf>,
) -> Result<(), Error> {
    let file_name_map = html_to_md
        .iter()
        .filter_map(|(k, v)| Some((k.file_name()?, v.file_name()?)))
        .collect::<HashMap<_, _>>();
    let output_dir = output_dir.as_ref();
    let src_dir = output_dir.join("src");
    for (_, (path, _)) in epub_doc.resources.clone().into_iter() {
        let mut content = match epub_doc.get_resource_by_path(&path) {
            Some(content) => content,
            None => continue, // unreachable
        };
        let target_path = if let Some(md_path) = html_to_md.get(&path) {
            // html file, convert to md
            let html = String::from_utf8(content.clone())?;
            let markdown = htmd::convert(&html)?;
            content = post_process_md(&markdown, &file_name_map).into_bytes();
            if md_path == Path::new("SUMMARY.md") {
                src_dir.join("_SUMMARY.md")
            } else {
                src_dir.join(md_path)
            }
        } else {
            // other file, just copy
            src_dir.join(&path)
        };
        // write to target path
        if let Some(parent) = target_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(target_path, content)?;
    }
    Ok(())
}

/// Capture the `{link}` without `#`, eg:
/// ```
/// [ABC]({abc.html}#xxx)
/// [ABC]({abc.html})
/// ```
static LINK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"\[[^\]]+\]\((?P<link>[^#)]+)(#[^)]+)?\)"#).expect("unreachable")
});
/// Match the URL link, eg:
/// ```
/// https://www.example.com\
/// ```
static URL_LINK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-z][a-z0-9+.-]*:").expect("unreachable"));

fn post_process_md(markdown: &str, file_name_map: &HashMap<&OsStr, &OsStr>) -> String {
    LINK.replace_all(markdown, |caps: &Captures| {
        // replace [ABC](abc.html#xxx) to [ABC](abc.md#xxx)
        let origin = &caps[0];
        let link = &caps["link"];
        // Don't modify links with schemes like `https`.
        if URL_LINK.is_match(link) {
            return origin.to_string();
        }
        let html_file_name = match Path::new(&link).file_name() {
            Some(link) => link,
            None => return origin.to_string(),
        };
        if let Some(md_file_name) = file_name_map.get(html_file_name) {
            origin.replace(
                &*html_file_name.to_string_lossy(),
                &md_file_name.to_string_lossy(),
            )
        } else {
            origin.to_string()
        }
    })
    .to_string()
}

fn write_book_toml(
    output_dir: impl AsRef<Path>,
    title: &str,
    creator: Option<String>,
) -> io::Result<()> {
    let output_dir = output_dir.as_ref();
    let author = match creator {
        Some(creator) => format!("author = \"{creator}\"\n"),
        None => "".to_string(),
    };
    let toml_content = format!("[book]\ntitle = \"{title}\"\n{author}",);
    fs::write(output_dir.join("book.toml"), toml_content)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_replace_links() {
        let markdown = r"[hello](hello.html#xxx) [hi](hi.xhtml)";
        let markdown = LINK.replace_all(&markdown, |caps: &Captures| {
            let link = &caps["link"];
            caps[0].replace(link, "link.md")
        });
        assert_eq!(markdown, "[hello](link.md#xxx) [hi](link.md)");
    }
}
