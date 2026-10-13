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
    writer.start_file("images/cover.bin", options)?;
    writer.write_all(&vec![0x5a; 1024 * 1024])?;
    writer.set_comment("preserve this archive comment")?;

    writer.finish()?;
    Ok(())
}

#[test]
fn output_preserves_unchanged_entries_and_has_a_valid_mimetype() -> Result<(), Box<dyn Error>> {
    use std::io::{Seek, SeekFrom};
    let temp = tempdir()?;
    let input = temp.path().join("sample.epub");
    build_sample_epub(&input)?;
    assert_cmd::Command::cargo_bin("fixepub")?
        .arg(&input)
        .assert()
        .success();
    let output = temp.path().join("sample-fixed.epub");
    let mut before = ZipArchive::new(File::open(&input)?)?;
    let mut after = ZipArchive::new(File::open(&output)?)?;
    assert_eq!(before.comment(), after.comment());
    {
        let mut mimetype = after.by_index(0)?;
        assert_eq!(mimetype.name()?.as_ref(), "mimetype");
        assert_eq!(mimetype.compression(), zip::CompressionMethod::Stored);
        assert!(mimetype.extra_data_fields().next().is_none());
        let mut content = String::new();
        mimetype.read_to_string(&mut content)?;
        assert_eq!(content, "application/epub+zip");
    }
    for name in [
        "chapter1.xhtml",
        "images/cover.bin",
        "META-INF/container.xml",
    ] {
        let original = before.by_name(name)?;
        let copied = after.by_name(name)?;
        assert_eq!(original.crc32(), copied.crc32());
        assert_eq!(original.last_modified(), copied.last_modified());
        assert_eq!(original.compression(), copied.compression());
        let mut original_bytes = vec![0; original.compressed_size() as usize];
        let mut copied_bytes = vec![0; copied.compressed_size() as usize];
        let mut original_file = File::open(&input)?;
        original_file.seek(SeekFrom::Start(original.data_start().unwrap()))?;
        original_file.read_exact(&mut original_bytes)?;
        let mut copied_file = File::open(&output)?;
        copied_file.seek(SeekFrom::Start(copied.data_start().unwrap()))?;
        copied_file.read_exact(&mut copied_bytes)?;
        assert_eq!(
            original_bytes, copied_bytes,
            "compressed data changed for {name}"
        );
    }
    for name in ["nav.xhtml", "content.opf"] {
        let mut content = Vec::new();
        after.by_name(name)?.read_to_end(&mut content)?;
        for event in xml::EventReader::new(content.as_slice()) {
            event?;
        }
    }
    Ok(())
}

#[test]
fn failed_repair_never_replaces_existing_output_or_leaves_partial_files(
) -> Result<(), Box<dyn Error>> {
    let temp = tempdir()?;
    let input = temp.path().join("sample.epub");
    build_epub(&input, b"<html><body/><unclosed>")?;
    let original_input = std::fs::read(&input)?;
    let output = temp.path().join("sample-fixed.epub");
    std::fs::write(&output, b"existing output")?;
    let mut command = assert_cmd::Command::cargo_bin("fixepub")?;
    command.arg("--force").arg(&input).assert().failure();
    assert_eq!(std::fs::read(&output)?, b"existing output");
    assert_eq!(std::fs::read(&input)?, original_input);
    assert_eq!(std::fs::read_dir(temp.path())?.count(), 2);
    Ok(())
}

#[test]
fn failed_repair_does_not_create_output() -> Result<(), Box<dyn Error>> {
    let temp = tempdir()?;
    let input = temp.path().join("sample.epub");
    build_epub(&input, b"<html><body/><unclosed>")?;
    assert_cmd::Command::cargo_bin("fixepub")?
        .arg(&input)
        .assert()
        .failure();
    assert_eq!(std::fs::read_dir(temp.path())?.count(), 1);
    Ok(())
}

#[test]
fn overwrite_requires_force_and_language_can_be_selected() -> Result<(), Box<dyn Error>> {
    let temp = tempdir()?;
    let input = temp.path().join("sample.epub");
    build_sample_epub(&input)?;
    let output = temp.path().join("sample-fixed.epub");
    std::fs::write(&output, b"existing output")?;
    assert_cmd::Command::cargo_bin("fixepub")?
        .arg(&input)
        .assert()
        .failure();
    assert_eq!(std::fs::read(&output)?, b"existing output");
    assert_cmd::Command::cargo_bin("fixepub")?
        .args(["--force", "--language", "fr-CA"])
        .arg(&input)
        .assert()
        .success();
    let mut archive = ZipArchive::new(File::open(output)?)?;
    let mut opf = String::new();
    archive.by_name("content.opf")?.read_to_string(&mut opf)?;
    assert!(opf.contains("<dc:language>fr-CA</dc:language>"));
    Ok(())
}

#[test]
fn cli_rejects_no_inputs_and_invalid_language() -> Result<(), Box<dyn Error>> {
    assert_cmd::Command::cargo_bin("fixepub")?
        .assert()
        .failure();
    assert_cmd::Command::cargo_bin("fixepub")?
        .args(["--language", "invalid", "missing.epub"])
        .assert()
        .failure();
    Ok(())
}

#[test]
fn dotted_filenames_keep_the_entire_stem() -> Result<(), Box<dyn Error>> {
    let temp = tempdir()?;
    let input = temp.path().join("book.second.edition.epub");
    build_sample_epub(&input)?;
    assert_cmd::Command::cargo_bin("fixepub")?
        .arg(&input)
        .assert()
        .success();
    assert!(temp.path().join("book.second.edition-fixed.epub").exists());
    Ok(())
}

#[cfg(target_os = "linux")]
#[test]
fn non_utf8_filenames_are_supported() -> Result<(), Box<dyn Error>> {
    use std::os::unix::ffi::OsStrExt;
    let temp = tempdir()?;
    let input = temp
        .path()
        .join(std::ffi::OsStr::from_bytes(b"book\xff.epub"));
    build_sample_epub(&input)?;
    assert_cmd::Command::cargo_bin("fixepub")?
        .arg(&input)
        .assert()
        .success();
    assert!(temp
        .path()
        .join(std::ffi::OsStr::from_bytes(b"book\xff-fixed.epub"))
        .exists());
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
