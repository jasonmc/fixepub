use clap::Parser;
use scraper::{Html, Selector};
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use zip::{write::FileOptions, ZipArchive, ZipWriter};

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[arg(last = true)]
    filenames: Vec<String>,
}

fn main() {
    let args = Args::parse();

    for f in args.filenames {
        let path = Path::new(&f);
        let stem = path.file_stem().unwrap();
        let new_stem = format!("{}-fixed", stem.to_str().unwrap());
        let new_path = change_file_stem(path, &new_stem);
        let np = new_path.as_path();

        println!("Fixing {} as {}", f, np.to_string_lossy());
        fix(&f, np)
    }

    println!("Done");
}

fn change_file_stem(original_path: &Path, new_stem: &str) -> PathBuf {
    let mut new_path = PathBuf::new();

    if let Some(parent) = original_path.parent() {
        new_path.push(parent);
    }

    // Create the new filename by combining the new stem with the original extension
    let new_filename = match original_path.extension() {
        Some(extension) => format!("{}.{}", new_stem, extension.to_string_lossy()),
        None => new_stem.to_string(),
    };

    new_path.push(new_filename);
    new_path
}

fn fix(filename: &str, output_filename: &Path) {
    let file = File::open(filename).expect("Failed to open EPUB file");
    let mut archive = ZipArchive::new(file).expect("Failed to read ZIP archive");

    let output_file = File::create(output_filename).expect("Failed to create output EPUB file");
    let mut output_zip = ZipWriter::new(output_file);

    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .expect("Failed to access file in ZIP archive");
        let file_name = file.name().to_string();

        let mut content = Vec::new();
        file.read_to_end(&mut content)
            .expect("Failed to read file content");
        let modified_content = process_content(&content);

        let options = FileOptions::default()
            .compression_method(file.compression())
            .unix_permissions(file.unix_mode().unwrap_or(0o755));

        output_zip
            .start_file(file_name, options)
            .expect("Failed to start file in ZIP");
        output_zip
            .write_all(&content)
            .expect("Failed to write file content");
    }

    output_zip.finish().expect("Failed to finalize ZIP archive");
}

fn process_content(content: &[u8]) -> Vec<u8> {
    content.to_vec()
}

fn process_html_content(content: &[u8]) -> Vec<u8> {
    let content_str = String::from_utf8_lossy(content);
    let document = Html::parse_document(&content_str);
    let body_selector = Selector::parse("body").unwrap();
    let body = document.select(&body_selector).next();

    let mut new_content = content_str.to_string();
    new_content.into_bytes()
}
