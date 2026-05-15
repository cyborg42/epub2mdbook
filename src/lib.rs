pub mod error;

use epub::doc::{EpubDoc, NavPoint};
use error::Error;
use htmd::element_handler::{HandlerResult, Handlers};
use mdbook_core::config::BookConfig;
use regex::{Captures, Regex};
use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::io::{Read, Seek};
use std::path::{Component, Path, PathBuf};
use std::sync::LazyLock;
use std::{fs, io};

/// Convert an EPUB file to MDBook format
///
/// # Arguments
///
/// * `epub_path` - Path to the input EPUB file
/// * `output_dir` - Path to the output directory
/// * `create_subdir` - If `true`, creates a subdirectory named after the EPUB file
///   (e.g., `output_dir/book_name/`). If `false`, outputs directly to `output_dir`.
pub fn convert_epub_to_mdbook(
    epub_path: impl AsRef<Path>,
    output_dir: impl AsRef<Path>,
    create_subdir: bool,
) -> Result<(), Error> {
    let epub_path = epub_path.as_ref();
    if !epub_path.is_file() {
        return Err(Error::NotAFile(epub_path.display().to_string()));
    }
    let mut output_dir = output_dir.as_ref().to_owned();
    if create_subdir {
        let book_name = epub_path
            .with_extension("")
            .file_name()
            .expect("unreachable")
            .to_string_lossy()
            .to_string();
        output_dir.push(book_name)
    }
    fs::create_dir_all(output_dir.join("src"))?;

    let mut epub_doc = EpubDoc::new(epub_path)?;
    let (summary_md, html_to_md) = generate_summary_md(&epub_doc);
    let html_to_title = collect_chapter_titles(&epub_doc, &html_to_md);
    extract_chapters_and_resources(&mut epub_doc, &output_dir, &html_to_md, &html_to_title)?;
    fs::write(output_dir.join("src/SUMMARY.md"), summary_md)?;
    write_book_toml(&epub_doc, &output_dir)?;
    Ok(())
}

fn epub_nav_to_md(
    nav: &NavPoint,
    indent: usize,
    html_to_md: &HashMap<PathBuf, PathBuf>,
) -> Option<String> {
    let (content_path, fragment) = split_fragment(&nav.content);
    let file = html_to_md.get(&content_path)?;
    let mut link = path_to_markdown_link(file);
    if let Some(fragment) = fragment {
        link.push('#');
        link.push_str(&fragment);
    }
    let mut md = format!("{}- [{}]({})\n", "  ".repeat(indent), nav.label, link);
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
///
/// # Returns
///
/// * `summary_md` - The SUMMARY.md content
/// * `html_to_md` - The file mapping from html to md
pub fn generate_summary_md<R: Read + Seek>(
    epub_doc: &EpubDoc<R>,
) -> (String, HashMap<PathBuf, PathBuf>) {
    let title = epub_doc.get_title();
    let mut summary_md = if let Some(title) = title {
        format!("# {}\n\n", title)
    } else {
        "".to_string()
    };
    let html_to_md = epub_doc
        .resources
        .iter()
        .filter(|(_, resource)| {
            ["application/xhtml+xml", "text/html"].contains(&resource.mime.as_str())
        })
        .map(|(_, resource)| (resource.path.clone(), resource.path.with_extension("md")))
        .collect::<HashMap<PathBuf, PathBuf>>();
    if epub_doc.toc.is_empty() {
        summary_md.push_str(&spine_to_md(epub_doc, &html_to_md));
    } else {
        for nav in &epub_doc.toc {
            if let Some(md) = epub_nav_to_md(nav, 0, &html_to_md) {
                summary_md.push_str(&md);
            }
        }
    }
    (summary_md, html_to_md)
}

fn spine_to_md<R: Read + Seek>(
    epub_doc: &EpubDoc<R>,
    html_to_md: &HashMap<PathBuf, PathBuf>,
) -> String {
    let mut md = String::new();
    for spine_item in &epub_doc.spine {
        if !spine_item.linear {
            continue;
        }
        let Some(resource) = epub_doc.resources.get(&spine_item.idref) else {
            continue;
        };
        let Some(file) = html_to_md.get(&resource.path) else {
            continue;
        };
        md.push_str(&format!(
            "- [{}]({})\n",
            path_to_title(&resource.path),
            path_to_markdown_link(file)
        ));
    }
    md
}

fn extract_chapters_and_resources<R: Read + Seek>(
    epub_doc: &mut EpubDoc<R>,
    output_dir: impl AsRef<Path>,
    html_to_md: &HashMap<PathBuf, PathBuf>,
    html_to_title: &HashMap<PathBuf, String>,
) -> Result<(), Error> {
    let src_dir = output_dir.as_ref().join("src");
    for (_, resource) in epub_doc.resources.clone() {
        let path = &resource.path;
        let mut content = match epub_doc.get_resource_by_path(path) {
            Some(content) => content,
            None => continue, // unreachable
        };
        let target_path = if let Some(md_path) = html_to_md.get(path) {
            // html file, convert to md
            let html = String::from_utf8(content.clone())?;
            let markdown = convert_epub_html_to_md(&html)?;
            let markdown =
                add_missing_chapter_title(&markdown, html_to_title.get(path).map(String::as_str));
            content = post_process_md(&markdown, path, html_to_md).into_bytes();
            if md_path == Path::new("SUMMARY.md") {
                src_dir.join("_SUMMARY.md")
            } else {
                src_dir.join(md_path)
            }
        } else {
            // other file, just copy
            src_dir.join(path)
        };
        // write to target path
        if let Some(parent) = target_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(target_path, content)?;
    }
    Ok(())
}

fn collect_chapter_titles<R: Read + Seek>(
    epub_doc: &EpubDoc<R>,
    html_to_md: &HashMap<PathBuf, PathBuf>,
) -> HashMap<PathBuf, String> {
    let mut html_to_title = HashMap::new();
    for nav in &epub_doc.toc {
        collect_nav_titles(nav, &mut html_to_title);
    }
    for spine_item in &epub_doc.spine {
        let Some(resource) = epub_doc.resources.get(&spine_item.idref) else {
            continue;
        };
        if html_to_md.contains_key(&resource.path) {
            html_to_title
                .entry(resource.path.clone())
                .or_insert_with(|| path_to_title(&resource.path));
        }
    }
    html_to_title
}

fn collect_nav_titles(nav: &NavPoint, html_to_title: &mut HashMap<PathBuf, String>) {
    let label = nav.label.trim();
    if !label.is_empty() {
        let path = strip_fragment(&nav.content);
        html_to_title
            .entry(path)
            .or_insert_with(|| label.to_string());
    }

    for child in &nav.children {
        collect_nav_titles(child, html_to_title);
    }
}

fn convert_epub_html_to_md(html: &str) -> io::Result<String> {
    htmd::HtmlToMarkdown::builder()
        .skip_tags(vec!["head"])
        .add_handler(
            vec![
                "a",
                "article",
                "aside",
                "blockquote",
                "body",
                "div",
                "figcaption",
                "figure",
                "h1",
                "h2",
                "h3",
                "h4",
                "h5",
                "h6",
                "li",
                "main",
                "nav",
                "p",
                "section",
                "span",
                "td",
                "th",
            ],
            preserve_id_handler,
        )
        .build()
        .convert(html)
}

fn preserve_id_handler(handlers: &dyn Handlers, element: htmd::Element) -> Option<HandlerResult> {
    let id = element
        .attrs
        .iter()
        .find(|attr| &*attr.name.local == "id")
        .map(|attr| attr.value.to_string())
        .filter(|id| !id.trim().is_empty());
    let mut result = handlers.fallback(element)?;
    if let Some(id) = id {
        let content = result.content.trim_start_matches('\n');
        result.content = format!("\n\n<a id=\"{}\"></a>\n\n{}", escape_attr(&id), content);
    }
    Some(result)
}

fn add_missing_chapter_title(markdown: &str, title: Option<&str>) -> String {
    let title = match title.map(str::trim).filter(|title| !title.is_empty()) {
        Some(title) => title,
        None => return markdown.to_string(),
    };
    if starts_with_markdown_heading(markdown) {
        return markdown.to_string();
    }

    let markdown = markdown.trim_start_matches('\n');
    if markdown.is_empty() {
        format!("# {title}")
    } else {
        format!("# {title}\n\n{markdown}")
    }
}

fn starts_with_markdown_heading(markdown: &str) -> bool {
    for line in markdown.lines().filter(|line| !line.trim().is_empty()) {
        if is_html_anchor(line) {
            continue;
        }
        return is_atx_heading(line);
    }
    false
}

fn is_atx_heading(line: &str) -> bool {
    let trimmed = line.trim_start_matches(' ');
    if line.len() - trimmed.len() > 3 {
        return false;
    }

    let hashes = trimmed.bytes().take_while(|byte| *byte == b'#').count();
    if !(1..=6).contains(&hashes) {
        return false;
    }

    let rest = &trimmed[hashes..];
    rest.is_empty() || rest.starts_with(' ') || rest.starts_with('\t')
}

fn is_html_anchor(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with("<a ")
        && trimmed.ends_with("></a>")
        && (trimmed.contains(" id=") || trimmed.contains(" name="))
}

fn strip_fragment(path: &Path) -> PathBuf {
    split_fragment(path).0
}

fn split_fragment(path: &Path) -> (PathBuf, Option<String>) {
    let path = path.to_string_lossy();
    match path.split_once('#') {
        Some((path, fragment)) => (PathBuf::from(path), Some(fragment.to_string())),
        None => (PathBuf::from(path.as_ref()), None),
    }
}

fn path_to_title(path: &Path) -> String {
    path.file_stem()
        .and_then(OsStr::to_str)
        .map(|stem| stem.replace(['-', '_'], " "))
        .filter(|title| !title.trim().is_empty())
        .unwrap_or_else(|| path_to_markdown_link(path))
}

fn resolve_relative_path(current_file: &Path, link: &str) -> PathBuf {
    let link_path = Path::new(link);
    let mut resolved = if link_path.is_absolute() {
        PathBuf::new()
    } else {
        current_file
            .parent()
            .unwrap_or_else(|| Path::new(""))
            .to_owned()
    };

    for component in link_path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                resolved.pop();
            }
            Component::Normal(part) => resolved.push(part),
            Component::RootDir | Component::Prefix(_) => {}
        }
    }

    resolved
}

fn relative_path(from_file: &Path, to_file: &Path) -> PathBuf {
    let from_dir = from_file.parent().unwrap_or_else(|| Path::new(""));
    let from = normalized_components(from_dir);
    let to = normalized_components(to_file);
    let common_len = from
        .iter()
        .zip(to.iter())
        .take_while(|(left, right)| left == right)
        .count();

    let mut relative = PathBuf::new();
    for _ in common_len..from.len() {
        relative.push("..");
    }
    for component in &to[common_len..] {
        relative.push(component);
    }
    relative
}

fn normalized_components(path: &Path) -> Vec<OsString> {
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            Component::CurDir | Component::RootDir | Component::Prefix(_) => {}
            Component::ParentDir => {
                components.pop();
            }
            Component::Normal(part) => components.push(part.to_os_string()),
        }
    }
    components
}

fn path_to_markdown_link(path: &Path) -> String {
    let parts = path
        .components()
        .filter_map(|component| match component {
            Component::CurDir => Some(".".to_string()),
            Component::ParentDir => Some("..".to_string()),
            Component::Normal(part) => Some(part.to_string_lossy().to_string()),
            Component::RootDir | Component::Prefix(_) => None,
        })
        .collect::<Vec<_>>();
    parts.join("/")
}

fn escape_attr(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Capture the `{link}` without `#`, eg:
/// ```text
/// [ABC]({abc.html}#xxx)
/// [ABC]({abc.html})
/// ```
static LINK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"\[[^\]]+\]\((?P<link>[^#)]+)(?P<fragment>#[^)]+)?\)"#).expect("unreachable")
});
/// Match the URL link, eg:
/// ```text
/// https://www.example.com\
/// ```
static URL_LINK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-z][a-z0-9+.-]*:").expect("unreachable"));

fn post_process_md(
    markdown: &str,
    current_html_path: &Path,
    html_to_md: &HashMap<PathBuf, PathBuf>,
) -> String {
    LINK.replace_all(markdown, |caps: &Captures| {
        // replace [ABC](abc.html#xxx) to [ABC](abc.md#xxx)
        let origin = &caps[0];
        let link = &caps["link"];
        // Don't modify links with schemes like `https`.
        if URL_LINK.is_match(link) {
            return origin.to_string();
        }
        let resolved_path = resolve_relative_path(current_html_path, link);
        if let Some(md_path) = html_to_md.get(&resolved_path) {
            let current_md_path = html_to_md
                .get(current_html_path)
                .cloned()
                .unwrap_or_else(|| current_html_path.with_extension("md"));
            let replacement = path_to_markdown_link(&relative_path(&current_md_path, md_path));
            origin.replace(link, &replacement)
        } else {
            origin.to_string()
        }
    })
    .to_string()
}

fn write_book_toml<R: Read + Seek>(
    epub_doc: &EpubDoc<R>,
    output_dir: impl AsRef<Path>,
) -> io::Result<()> {
    let output_dir = output_dir.as_ref();
    let title = epub_doc.get_title();
    let authors = epub_doc
        .metadata
        .iter()
        .filter(|m| m.property == "creator")
        .map(|m| m.value.clone())
        .collect::<Vec<_>>();
    let description = epub_doc
        .mdata("description")
        .and_then(|m| htmd::convert(&m.value).ok());
    let lang = epub_doc
        .mdata("language")
        .or_else(|| epub_doc.mdata("lang"))
        .map(|m| m.value.clone());
    let mut config = BookConfig::default();
    config.title = title;
    config.authors = authors;
    config.description = description;
    config.src = PathBuf::from("src");
    config.language = lang;
    let toml_content = format!("[book]\n{}", toml::to_string(&config).expect("unreachable"));
    fs::write(output_dir.join("book.toml"), toml_content)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_replace_links() {
        let markdown = r"[hello](hello.html#xxx) [hi](hi.xhtml)";
        let html_to_md = HashMap::from([
            (
                PathBuf::from("text/current.xhtml"),
                PathBuf::from("text/current.md"),
            ),
            (
                PathBuf::from("text/hello.html"),
                PathBuf::from("text/hello.md"),
            ),
            (PathBuf::from("text/hi.xhtml"), PathBuf::from("text/hi.md")),
        ]);

        let markdown = post_process_md(markdown, Path::new("text/current.xhtml"), &html_to_md);

        assert_eq!(markdown, "[hello](hello.md#xxx) [hi](hi.md)");
    }

    #[test]
    fn test_replace_links_resolves_relative_paths() {
        let markdown = r"[next](../part2/index.xhtml#target) [same](chapter.xhtml) [site](https://example.com/index.xhtml)";
        let html_to_md = HashMap::from([
            (
                PathBuf::from("OPS/part1/current.xhtml"),
                PathBuf::from("OPS/part1/current.md"),
            ),
            (
                PathBuf::from("OPS/part1/chapter.xhtml"),
                PathBuf::from("OPS/part1/chapter.md"),
            ),
            (
                PathBuf::from("OPS/part2/index.xhtml"),
                PathBuf::from("OPS/part2/index.md"),
            ),
        ]);

        let markdown = post_process_md(markdown, Path::new("OPS/part1/current.xhtml"), &html_to_md);

        assert_eq!(
            markdown,
            "[next](../part2/index.md#target) [same](chapter.md) [site](https://example.com/index.xhtml)"
        );
    }

    #[test]
    fn test_nav_fragment_is_preserved_in_summary() {
        let nav = NavPoint {
            label: "Section I".to_string(),
            content: PathBuf::from("epub/text/chapter.xhtml#section-1"),
            children: Vec::new(),
            play_order: Some(1),
        };
        let html_to_md = HashMap::from([(
            PathBuf::from("epub/text/chapter.xhtml"),
            PathBuf::from("epub/text/chapter.md"),
        )]);

        let markdown = epub_nav_to_md(&nav, 0, &html_to_md).unwrap();

        assert_eq!(markdown, "- [Section I](epub/text/chapter.md#section-1)\n");
    }

    #[test]
    fn test_epub_html_conversion_skips_head_metadata() {
        let html = r#"
            <html>
                <head>
                    <title>A Scandal in Bohemia</title>
                    <script>console.log("metadata");</script>
                    <style>body { color: red; }</style>
                </head>
                <body>
                    <article>
                        <h2>A Scandal in Bohemia</h2>
                        <p>To Sherlock Holmes she is always <em>the</em> woman.</p>
                    </article>
                </body>
            </html>
        "#;
        let title = "A Scandal in Bohemia".to_string();

        let markdown = convert_epub_html_to_md(html).unwrap();
        let markdown = add_missing_chapter_title(&markdown, Some(&title));

        assert_eq!(
            markdown,
            "## A Scandal in Bohemia\n\nTo Sherlock Holmes she is always *the* woman."
        );
    }

    #[test]
    fn test_epub_html_conversion_preserves_ids_as_anchors() {
        let html = r#"
            <html>
                <body>
                    <section id="chapter-1">
                        <h2>Chapter One</h2>
                        <p>Opening paragraph.</p>
                    </section>
                </body>
            </html>
        "#;

        let markdown = convert_epub_html_to_md(html).unwrap();
        let markdown = add_missing_chapter_title(&markdown, Some("Chapter One"));

        assert!(markdown.starts_with("<a id=\"chapter-1\"></a>\n\n## Chapter One"));
    }

    #[test]
    fn test_missing_body_title_uses_toc_label() {
        let html = r#"
            <html>
                <head>
                    <title>Head Metadata Title</title>
                </head>
                <body>
                    <p>Opening paragraph.</p>
                </body>
            </html>
        "#;
        let title = "Chapter One".to_string();

        let markdown = convert_epub_html_to_md(html).unwrap();
        let markdown = add_missing_chapter_title(&markdown, Some(&title));

        assert_eq!(markdown, "# Chapter One\n\nOpening paragraph.");
    }
}
