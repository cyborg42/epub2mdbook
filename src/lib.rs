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
/// * `output_dir` - The path to the output directory, pwd by default
///
pub fn convert_epub_to_mdbook(
    epub_path: impl AsRef<Path>,
    output_dir: Option<impl AsRef<Path>>,
) -> Result<(), Error> {
    let epub_path = epub_path.as_ref();
    if !epub_path.is_file() {
        return Err(Error::NotAFile(epub_path.display().to_string()));
    }
    let book_name = epub_path.with_extension("");
    let book_name = book_name
        .file_name()
        .expect("unreachable")
        .to_string_lossy()
        .to_string();
    let output_dir = match output_dir {
        Some(output_dir) => output_dir.as_ref().join(&book_name),
        None => PathBuf::from(".").join(&book_name),
    };
    fs::create_dir_all(output_dir.join("src"))?;

    let mut doc = EpubDoc::new(epub_path)?;
    let title = doc
        .metadata
        .get("title")
        .and_then(|v| v.first().cloned())
        .unwrap_or(book_name);
    let creator = doc.metadata.get("creator").and_then(|v| v.first().cloned());
    let (toc, html_to_md) = toc_to_md(&doc, &title);
    extract_chapters_and_resources(&mut doc, &output_dir, &html_to_md)?;
    fs::write(output_dir.join("src/SUMMARY.md"), toc)?;
    write_book_toml(&output_dir, &title, creator)?;
    Ok(())
}

fn nav_to_md(
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
        if let Some(child_md) = nav_to_md(child, indent + 1, html_to_md) {
            md.push_str(&child_md);
        }
    }
    Some(md)
}

/// Convert the table of contents to SUMMARY.md
///
/// # Arguments
///
/// * `doc` - The EPUB document
/// * `title` - The title of the book
///
/// # Returns
///
/// * `summary_md` - The SUMMARY.md content
/// * `html_to_md` - The file mapping from html to md
pub fn toc_to_md<R: Read + Seek>(
    doc: &EpubDoc<R>,
    title: &str,
) -> (String, HashMap<PathBuf, PathBuf>) {
    let toc = doc.toc.clone();

    let mut summary_md = format!("# {}\n\n", title);
    let html_to_md = doc
        .resources
        .iter()
        .filter(|(_, (_, mime))| mime == "application/xhtml+xml")
        .map(|(_, (path, _))| (path.clone(), path.with_extension("md")))
        .collect::<HashMap<PathBuf, PathBuf>>();
    for nav in toc {
        if let Some(md) = nav_to_md(&nav, 0, &html_to_md) {
            summary_md.push_str(&md);
        }
    }
    (summary_md, html_to_md)
}

fn extract_chapters_and_resources<R: Read + Seek>(
    doc: &mut EpubDoc<R>,
    output_dir: impl AsRef<Path>,
    html_to_md: &HashMap<PathBuf, PathBuf>,
) -> Result<(), Error> {
    let file_name_map = html_to_md
        .iter()
        .filter_map(|(k, v)| Some((k.file_name()?, v.file_name()?)))
        .collect::<HashMap<_, _>>();
    let output_dir = output_dir.as_ref();
    let src_dir = output_dir.join("src");
    for (_, (path, _)) in doc.resources.clone().into_iter() {
        let content = match doc.get_resource_by_path(&path) {
            Some(content) => content,
            None => continue, // unreachable
        };
        if let Some(md_path) = html_to_md.get(&path) {
            // html file, convert to md
            let target_path = if md_path == Path::new("SUMMARY.md") {
                src_dir.join("_SUMMARY.md")
            } else {
                src_dir.join(md_path)
            };
            if let Some(parent) = target_path.parent() {
                fs::create_dir_all(parent)?;
            }
            let html = String::from_utf8(content)?;
            let markdown = html2md::parse_html(&html);
            let markdown = post_process_md(&markdown, &file_name_map);
            fs::write(target_path, markdown)?;
        } else {
            // other file, just copy
            let target_path = src_dir.join(&path);
            if let Some(parent) = target_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(target_path, content)?;
        }
    }
    Ok(())
}

/// Capture the `{link}` without `#`, eg:
/// ```
/// [ABC]({abc.html}#xxx)
/// [ABC]({abc.html})
/// ```
static LINK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"\[[^\]]+\]\(([^#)]+)(?:#[^)]+)?\)"#).expect("unreachable"));
static EMPTY_LINK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"\[([^\]]+)\]\(\)"#).expect("unreachable"));
static URL_LINK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-z][a-z0-9+.-]*:").expect("unreachable"));
fn post_process_md(markdown: &str, file_name_map: &HashMap<&OsStr, &OsStr>) -> String {
    let markdown = LINK
        .replace_all(markdown, |caps: &Captures| {
            // replace [ABC](abc.html#xxx) to [ABC](abc.md#xxx)
            let origin = &caps[0];
            let link = &caps[1];
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
                    &html_file_name.to_string_lossy().to_string(),
                    &md_file_name.to_string_lossy(),
                )
            } else {
                origin.to_string()
            }
        })
        .replace(r"![]()", "")
        .replace(r"[]()", "");

    EMPTY_LINK
        .replace_all(&markdown, |caps: &Captures| caps[1].to_string())
        .to_string()
}

fn write_book_toml(
    output_dir: impl AsRef<Path>,
    title: &str,
    creator: Option<String>,
) -> io::Result<()> {
    let output_dir = output_dir.as_ref();
    let creator = match creator {
        Some(creator) => format!("author = \"{creator}\"\n"),
        None => "".to_string(),
    };
    let toml_content = format!("[book]\ntitle = \"{title}\"\n{creator}",);
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
            let link = caps[1].to_string();
            caps[0].replace(&link, "hello.md")
        });
        assert_eq!(markdown, "[hello](hello.md#xxx) [hi](hello.md)");
    }
}
