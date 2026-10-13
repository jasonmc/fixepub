use std::error::Error;
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use tempfile::tempdir;
use zip::write::SimpleFileOptions;
use zip::{ZipArchive, ZipWriter};

#[test]
fn fixes_epub_end_to_end() -> Result<(), Box<dyn Error>> {
    let temp = tempdir()?;
    let input_path = temp.path().join("sample.epub");
    build_sample_epub(&input_path)?;

    let mut cmd = assert_cmd::Command::cargo_bin("fixepub")?;
    cmd.arg("--").arg(&input_path);
    cmd.assert().success();

    let output_path = temp.path().join("sample-fixed.epub");
    assert!(output_path.exists(), "expected output EPUB to be created");

    let file = File::open(&output_path)?;
    let mut archive = ZipArchive::new(file)?;

    let mut nav = String::new();
    archive.by_name("nav.xhtml")?.read_to_string(&mut nav)?;
    assert!(
        nav.lines()
            .next()
            .map(|line| line.contains(r#"<?xml version="1.0" encoding="UTF-8"?>"#))
            .unwrap_or(false),
        "expected nav.xhtml to start with an XML declaration, got {nav:?}"
    );
    assert!(
        !nav.contains("#chap1"),
        "expected fragment identifiers to be stripped from links: {nav}"
    );
    assert!(
        nav.contains(r#"<a href="chapter1.xhtml">Go to chapter</a>"#),
        "expected link to point to chapter1.xhtml without fragment: {nav}"
    );
    assert!(
        !nav.contains("<img"),
        "expected stray <img> without src to be removed: {nav}"
    );

    let mut opf = String::new();
    archive.by_name("content.opf")?.read_to_string(&mut opf)?;
    assert!(
        opf.contains("<dc:language>en</dc:language>"),
        "expected OPF language to be normalized to English: {opf}"
    );

    Ok(())
}

fn build_sample_epub(path: &Path) -> Result<(), Box<dyn Error>> {
    build_epub(
        path,
        br#"<html xmlns="http://www.w3.org/1999/xhtml">
  <body>
    <a href="chapter1.xhtml#chap1">Go to chapter</a>
    <img alt="cover"/>
  </body>
</html>"#,
    )
}

fn build_epub(path: &Path, nav: &[u8]) -> Result<(), Box<dyn Error>> {
    let file = File::create(path)?;
    let mut writer = ZipWriter::new(file);
    let options = SimpleFileOptions::default();

    writer.start_file("META-INF/container.xml", options)?;
    writer.write_all(
        br#"<?xml version="1.0" encoding="UTF-8"?>
<container>
  <rootfiles>
    <rootfile full-path="content.opf"/>
  </rootfiles>
</container>"#,
    )?;

    writer.start_file("content.opf", options)?;
    writer.write_all(
        br#"<package xmlns="http://www.idpf.org/2007/opf" xmlns:dc="http://purl.org/dc/elements/1.1/">
  <metadata>
    <dc:language>xx-INVALID</dc:language>
  </metadata>
</package>"#,
    )?;

    writer.start_file("chapter1.xhtml", options)?;
    writer.write_all(
        br#"<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml">
  <body id="chap1">
    <p>Chapter 1</p>
  </body>
</html>"#,
    )?;

    writer.start_file("nav.xhtml", options)?;
    writer.write_all(nav)?;
    writer.finish()?;
    Ok(())
}

fn build_link_epub(path: &Path, chapters: &[(&str, &str)]) -> Result<(), Box<dyn Error>> {
    let mut writer = ZipWriter::new(File::create(path)?);
    let options = SimpleFileOptions::default();
    writer.start_file("META-INF/container.xml", options)?;
    writer.write_all(
        br#"<container><rootfiles><rootfile full-path="content.opf"/></rootfiles></container>"#,
    )?;
    writer.start_file("content.opf", options)?;
    writer.write_all(br#"<package xmlns="http://www.idpf.org/2007/opf"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:language>en</dc:language></metadata></package>"#)?;
    for (name, content) in chapters {
        writer.start_file(*name, options)?;
        writer.write_all(content.as_bytes())?;
    }
    writer.finish()?;
    Ok(())
}

#[test]
fn cli_resolves_dot_directory_bases_without_rewriting_other_fragments() -> Result<(), Box<dyn Error>>
{
    let temp = tempdir()?;
    let input = temp.path().join("sample.epub");
    build_link_epub(
        &input,
        &[
            (
                "OEBPS/nav.xhtml",
                r#"<html><body xml:base="chapters/."><a href="chapter.xhtml#body">Body</a><a href="chapter.xhtml#paragraph">Paragraph</a></body></html>"#,
            ),
            (
                "OEBPS/chapters/chapter.xhtml",
                r#"<html><body id="body"><p id="paragraph">Text</p></body></html>"#,
            ),
            (
                "OEBPS/chapter.xhtml",
                r#"<html><body id="paragraph"/></html>"#,
            ),
        ],
    )?;
    assert_cmd::Command::cargo_bin("fixepub")?
        .arg("--")
        .arg(&input)
        .assert()
        .success();
    let mut archive = ZipArchive::new(File::open(temp.path().join("sample-fixed.epub"))?)?;
    let mut nav = String::new();
    archive
        .by_name("OEBPS/nav.xhtml")?
        .read_to_string(&mut nav)?;
    assert!(nav.contains("href=\"chapter.xhtml\""));
    assert!(nav.contains("href=\"chapter.xhtml#paragraph\""));
    Ok(())
}

#[test]
fn cli_preserves_literal_archive_path_characters_during_link_repair() -> Result<(), Box<dyn Error>>
{
    let temp = tempdir()?;
    for (index, directory) in ["Part#1", "Part?1", "Part%1", "Part%23", "Part%2F"]
        .iter()
        .enumerate()
    {
        let input = temp.path().join(format!("sample{index}.epub"));
        let nav_path = format!("OEBPS/{directory}/nav.xhtml");
        let chapter_path = format!("OEBPS/{directory}/chapter.xhtml");
        build_link_epub(
            &input,
            &[
                (
                    &nav_path,
                    r#"<html><body><a href="chapter.xhtml#body">Chapter</a></body></html>"#,
                ),
                (&chapter_path, r#"<html><body id="body"/></html>"#),
            ],
        )?;
        assert_cmd::Command::cargo_bin("fixepub")?
            .arg("--")
            .arg(&input)
            .assert()
            .success();
        let mut archive = ZipArchive::new(File::open(
            temp.path().join(format!("sample{index}-fixed.epub")),
        )?)?;
        let mut nav = String::new();
        archive.by_name(&nav_path)?.read_to_string(&mut nav)?;
        assert!(nav.contains("href=\"chapter.xhtml\""), "{directory}: {nav}");
    }
    Ok(())
}
