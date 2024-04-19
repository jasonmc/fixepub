use clap::Parser;
use html5ever::tree_builder::TreeSink;
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

    //iterate over all xhtml to get body IDs
    let mut body_id_list: Vec<(String, String)> = Vec::new();
    for i in 0..archive.len() {
        let mut file = archive.by_index(i).expect("Failed to access");
        //println!("Filename: {}", file.name());
        //std::io::copy(&mut file, &mut std::io::stdout());
        let file_name = file.name().to_string();
        let ext = Path::new(&file_name)
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("");

        if ext == "html" || ext == "xhtml" {
            //let html = fs::read_to_string(file_path).expect("Unable to read file");
            let mut content = Vec::new();
            file.read_to_end(&mut content)
                .expect("Failed to read file content");
            let html = String::from_utf8_lossy(&content);
            let document = Html::parse_document(&html);
            let body_selector = Selector::parse("body").unwrap();
            let body = document.select(&body_selector).next();

            if let Some(body_element) = body {
                let body_id = body_element.value().attr("id").unwrap_or("");
                if !body_id.is_empty() {
                    let fname = Path::new(&file_name)
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("");
                    let link_target = format!("{}#{}", fname, body_id);
                    body_id_list.push((link_target, fname.to_string()));
                }
            }
        }
    }

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
            .write_all(&modified_content)
            .expect("Failed to write file content");
    }

    output_zip.finish().expect("Failed to finalize ZIP archive");
}

fn process_content(content: &[u8]) -> Vec<u8> {
    content.to_vec()
}

// fn process_html_content(content: &[u8]) -> Vec<u8> {
//     let content_str = String::from_utf8_lossy(content);
//     let document = Html::parse_document(&content_str);
//     let body_selector = Selector::parse("body").unwrap();
//     let body = document.select(&body_selector).next();

//     document.html().into_bytes()
// }

fn fix_body_id_link(
    file_path: String,
    content: &[u8],
    body_id_list: Vec<(String, String)>,
) -> Vec<u8> {
    let mut html = String::from_utf8_lossy(&content).to_string();
    for (src, target) in body_id_list.iter() {
        if html.contains(src) {
            html = html.replace(src, target);
        }
    }
    html.into_bytes()
}

fn fix_encoding(file_path: String, content: &[u8]) -> Vec<u8> {
    let encoding = r#"<?xml version="1.0" encoding="utf-8"?>"#;
    let ext = Path::new(&file_path)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    if ext == "html" || ext == "xhtml" {
        let content_str = String::from_utf8_lossy(content);
        let trimmed_html = content_str.trim_start();
        // Check if the beginning of the file content starts with a partial XML declaration
        if !trimmed_html.starts_with(r#"<?xml version="1.0" encoding="#) {
            return format!("{}\n{}", encoding, trimmed_html).into_bytes();
        }
    }
    content.to_vec()
}

fn fix_stray_img(file_path: String, content: &[u8]) -> Vec<u8> {
    let html = String::from_utf8_lossy(&content).to_string();
    let mut document = Html::parse_document(&html);
    let selector = Selector::parse("img").unwrap();
    let mut stray_imgs = Vec::new();

    for img in document.select(&selector) {
        if img.value().attr("src").is_none() {
            stray_imgs.push(img.id());
        }
    }

    if !stray_imgs.is_empty() {
        for img in stray_imgs {
            document.remove_from_parent(&img);
        }
    }
    document.html().into_bytes()
}
