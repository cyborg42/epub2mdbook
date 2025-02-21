use epub::doc::{EpubDoc, NavPoint};
use html2md::parse_html;
use std::collections::HashMap;
use std::fs;
use std::io::{Read, Seek};
use std::path::{Path, PathBuf};

pub fn convert_epub_to_mdbook(
    epub_path: impl AsRef<Path>,
    output_dir: Option<impl AsRef<Path>>,
) -> anyhow::Result<()> {
    let book_name = epub_path.as_ref().with_extension("");
    let book_name = book_name.file_name().unwrap().to_str().unwrap();
    let pwd = PathBuf::from(".");
    let output_dir = match output_dir {
        Some(output_dir) => output_dir.as_ref().join(book_name),
        None => pwd.join(book_name),
    };

    fs::create_dir_all(output_dir.join("src"))?;

    let mut doc = EpubDoc::new(&epub_path)?;
    let title = if let Some(title) = doc.metadata.get("title") {
        title.first().unwrap_or(&book_name.to_string()).clone()
    } else {
        book_name.to_string()
    };
    let creator = doc.metadata.get("creator").and_then(|v| v.first().cloned());

    let (toc, html_files) = toc_to_md(&doc, &title)?;
    fs::write(output_dir.join("src/SUMMARY.md"), toc)?;

    extract_chapters_and_resources(&mut doc, &output_dir, &html_files)?;
    write_book_toml(&output_dir, &title, creator)?;
    Ok(())
}

pub fn nav_point_to_md(
    nav: &NavPoint,
    indent: usize,
    html_files: &HashMap<PathBuf, PathBuf>,
) -> Option<String> {
    let file = html_files.get(&nav.content)?;
    let mut md = format!(
        "{}- [{}]({})\n",
        "  ".repeat(indent),
        nav.label,
        file.to_str()?
    );
    for child in &nav.children {
        if let Some(child_md) = nav_point_to_md(child, indent + 1, html_files) {
            md.push_str(&child_md);
        }
    }
    Some(md)
}

pub fn toc_to_md<R: Read + Seek>(
    doc: &EpubDoc<R>,
    title: &str,
) -> anyhow::Result<(String, HashMap<PathBuf, PathBuf>)> {
    let toc = doc.toc.clone();

    let mut markdown = format!("# {}\n\n", title);
    let html_files = doc
        .resources
        .iter()
        .filter(|(_, (_, mime))| mime == "application/xhtml+xml")
        .map(|(_, (path, _))| (path.clone(), path.with_extension("md")))
        .collect::<HashMap<PathBuf, PathBuf>>();
    for nav in toc {
        if let Some(md) = nav_point_to_md(&nav, 0, &html_files) {
            markdown.push_str(&md);
        }
    }
    Ok((markdown, html_files))
}

pub fn extract_chapters_and_resources<R: Read + Seek>(
    doc: &mut EpubDoc<R>,
    output_dir: impl AsRef<Path>,
    html_files: &HashMap<PathBuf, PathBuf>,
) -> anyhow::Result<()> {
    let output_dir = output_dir.as_ref();
    let src_dir = output_dir.join("src");

    for (_, (path, _)) in doc.resources.clone().into_iter() {
        let content = match doc.get_resource_by_path(&path) {
            Some(content) => content,
            None => continue,
        };

        if let Some(path) = html_files.get(&path) {
            let target_path = src_dir.join(path);
            if let Some(parent) = target_path.parent() {
                fs::create_dir_all(parent)?;
            }
            let html = String::from_utf8(content)?;
            let markdown = parse_html(&html);
            fs::write(target_path, markdown)?;
        } else {
            let target_path = src_dir.join(&path);
            if let Some(parent) = target_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(target_path, content)?;
        }
    }
    Ok(())
}

pub fn write_book_toml(
    output_dir: impl AsRef<Path>,
    title: &str,
    creator: Option<String>,
) -> anyhow::Result<()> {
    let output_dir = output_dir.as_ref();
    let creator = match creator {
        Some(creator) => format!("author = \"{creator}\"\n"),
        None => "".to_string(),
    };
    let toml_content = format!("[book]\ntitle = \"{title}\"\n{creator}",);
    fs::write(output_dir.join("book.toml"), toml_content)?;
    Ok(())
}
