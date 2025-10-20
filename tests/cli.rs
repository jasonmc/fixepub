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
            .map(|line| line.contains(r#"<?xml version="1.0" encoding="utf-8"?>"#))
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
        opf.contains("<language>en</language>"),
        "expected OPF language to be normalized to English: {opf}"
    );

    Ok(())
}

fn build_sample_epub(path: &Path) -> Result<(), Box<dyn Error>> {
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
        br#"<package>
  <metadata>
    <language>xx-INVALID</language>
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
    writer.write_all(
        br#"<html xmlns="http://www.w3.org/1999/xhtml">
  <body>
    <a href="chapter1.xhtml#chap1">Go to chapter</a>
    <img alt="cover"/>
  </body>
</html>"#,
    )?;

    writer.finish()?;
    Ok(())
}
