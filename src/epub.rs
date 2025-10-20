use crate::encoding_matcher;
use indicatif::{ProgressBar, ProgressStyle};
use scraper::{Html, Selector};
use std::fs::File;
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use xmltree::{Element, EmitterConfig, XMLNode};
use zip::{write::SimpleFileOptions, ZipArchive, ZipWriter};

pub(crate) fn change_file_stem(original_path: &Path, new_stem: &str) -> PathBuf {
    let mut new_path = PathBuf::new();

    if let Some(parent) = original_path.parent() {
        new_path.push(parent);
    }

    let new_filename = match original_path.extension() {
        Some(extension) => format!("{}.{}", new_stem, extension.to_string_lossy()),
        None => new_stem.to_string(),
    };

    new_path.push(new_filename);
    new_path
}

pub(crate) fn fix(filename: &str, output_filename: &Path) {
    let file = File::open(filename).expect("Failed to open EPUB file");
    let mut archive = ZipArchive::new(file).expect("Failed to read ZIP archive");

    let output_file = File::create(output_filename).expect("Failed to create output EPUB file");
    let mut output_zip = ZipWriter::new(output_file);

    // iterate over all xhtml to get body IDs
    let mut body_id_list: Vec<(String, String)> = Vec::new();
    let mut opf_path = "".to_string();
    for i in 0..archive.len() {
        let mut file = archive.by_index(i).expect("Failed to access");
        let file_name = file.name().to_string();
        let ext = Path::new(&file_name)
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("");

        if ext == "html" || ext == "xhtml" {
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
        if file_name == "META-INF/container.xml" {
            let mut content = Vec::new();
            file.read_to_end(&mut content)
                .expect("Failed to read file content");
            opf_path = get_opf_filename(&content)
        }
    }

    let pb = ProgressBar::new(archive.len() as u64);
    pb.set_style(
        ProgressStyle::with_template("{spinner:.green} [{wide_bar:.cyan/blue}] {pos}/{len}")
            .unwrap(),
    );

    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .expect("Failed to access file in ZIP archive");
        let file_name = file.name().to_string();

        let mut content = Vec::new();
        file.read_to_end(&mut content)
            .expect("Failed to read file content");
        let modified_content = process_file(file_name.as_str(), &content, &body_id_list, &opf_path);

        let options = SimpleFileOptions::default()
            .compression_method(file.compression())
            .unix_permissions(file.unix_mode().unwrap_or(0o755));

        output_zip
            .start_file(file_name, options)
            .expect("Failed to start file in ZIP");
        output_zip
            .write_all(&modified_content)
            .expect("Failed to write file content");
        pb.inc(1);
    }

    output_zip.finish().expect("Failed to finalize ZIP archive");
    pb.finish_with_message("done");
}

fn process_file(
    file_path: &str,
    content: &[u8],
    body_id_list: &[(String, String)],
    opf_path: &str,
) -> Vec<u8> {
    fix_encoding(
        file_path,
        &fix_stray_img(
            file_path,
            &fix_book_language(
                file_path,
                &fix_body_id_link(file_path, content, body_id_list),
                opf_path,
            ),
        ),
    )
}

fn is_xhtml(file_path: &str) -> bool {
    matches!(
        Path::new(file_path).extension().and_then(|s| s.to_str()),
        Some("html" | "xhtml")
    )
}

fn fix_body_id_link(file_path: &str, content: &[u8], body_id_list: &[(String, String)]) -> Vec<u8> {
    if !is_xhtml(file_path) {
        return content.to_vec();
    }

    let mut html = String::from_utf8_lossy(&content).to_string();
    for (src, target) in body_id_list.iter() {
        if html.contains(src) {
            html = html.replace(src, target);
        }
    }
    html.into_bytes()
}

fn fix_encoding(file_path: &str, content: &[u8]) -> Vec<u8> {
    if is_xhtml(file_path) {
        let encoding = r#"<?xml version="1.0" encoding="utf-8"?>"#;
        let content_str = String::from_utf8_lossy(content);
        let trimmed_html = content_str.trim_start();

        // Check if the beginning of the file content starts with a partial XML declaration
        match encoding_matcher::is_xml_declaration(trimmed_html) {
            Ok((_, true)) => (),
            _ => {
                return format!("{}\n{}", encoding, trimmed_html).into_bytes();
            }
        }
    }
    content.to_vec()
}

fn fix_stray_img(file_path: &str, content: &[u8]) -> Vec<u8> {
    if !is_xhtml(file_path) {
        return content.to_vec();
    }

    let html = String::from_utf8_lossy(&content).to_string();
    let mut document = Html::parse_document(&html);
    let selector = Selector::parse("img").unwrap();

    let stray_imgs: Vec<_> = document
        .select(&selector)
        .filter(|img| img.value().attr("src").is_none())
        .map(|img| img.id())
        .collect();

    if !stray_imgs.is_empty() {
        for img in stray_imgs {
            document.tree.get_mut(img).unwrap().detach();
        }
        return document.html().into_bytes();
    }
    content.to_vec()
}

fn get_opf_filename(content: &[u8]) -> String {
    let container_xml = Element::parse(content)
        .map_err(|_| "Error parsing container.xml")
        .unwrap();
    container_xml
        .get_child("rootfiles")
        .and_then(|rf| rf.get_child("rootfile"))
        .and_then(|rf| rf.attributes.get("full-path"))
        .ok_or("Cannot find OPF file path in container.xml")
        .unwrap()
        .to_string()
}

fn fix_book_language(file_path: &str, content: &[u8], opf_path: &str) -> Vec<u8> {
    if file_path != opf_path {
        return content.to_vec();
    }
    let mut opf = Element::parse(content)
        .map_err(|_| "Error parsing OPF file")
        .unwrap();

    let metadata = opf
        .get_mut_child("metadata")
        .ok_or("No metadata in OPF file")
        .unwrap();

    let changed = fix_language(metadata);

    if !changed {
        return content.to_vec();
    }

    let config = EmitterConfig::new()
        .perform_indent(true)
        .normalize_empty_elements(false);

    let mut buf = BufWriter::new(Vec::new());
    opf.write_with_config(&mut buf, config)
        .map_err(|_| "Error serializing OPF file")
        .unwrap();

    buf.into_inner().unwrap()
}

fn simplify_language(lang: &str) -> String {
    lang.split('-').next().unwrap().to_lowercase()
}

const ALLOWED_LANGUAGES: &[&str] = &[
    "af", "afr", "ar", "ara", "baq", "br", "bre", "ca", "cat", "chi", "co", "cor", "cos", "cy",
    "cym", "da", "dan", "de", "deu", "dut", "en", "eng", "es", "eu", "eus", "fi", "fin", "fr",
    "fra", "fre", "frr", "fry", "fy", "ga", "gd", "ger", "gla", "gle", "gl", "glg", "glv", "gsw",
    "gu", "guj", "gv", "hi", "hin", "is", "ice", "isl", "it", "ita", "ja", "jpn", "kw", "lb",
    "ltz", "mal", "mar", "ml", "mr", "nb", "nld", "nl", "nn", "nno", "nob", "nor", "oc", "oci",
    "pl", "por", "pt", "rm", "roh", "sco", "spa", "stq", "sv", "swe", "ta", "tam", "wel", "zho",
    "zh",
];

fn fix_language(metadata: &mut Element) -> bool {
    // Check if 'dc:language' exists and extract the language, if present
    let mut language_tag = metadata.get_mut_child("language");

    let mut language = language_tag
        .as_mut()
        .and_then(|lt| lt.get_text().map(String::from))
        .unwrap_or_default();

    let s = simplify_language(language.as_str());
    if !ALLOWED_LANGUAGES.contains(&s.as_str()) {
        println!(
            "Language {} is not supported. Asking for a valid language.",
            language
        );
        language = "en".to_string(); // TODO: replace with flag.
    } else {
        return false;
    }

    if language_tag.is_none() {
        println!("Language tag is missing. {:?}", metadata);
        let mut new_language_tag = Element::new("dc:language");
        new_language_tag.children.clear();
        new_language_tag
            .children
            .push(XMLNode::Text(language.clone()));
        metadata.children.push(XMLNode::Element(new_language_tag));
    } else {
        let t = language_tag.unwrap();
        t.children.clear();
        t.children.push(XMLNode::Text(language.clone()));
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_file_works() {
        let content = "b";
        let result = process_file("a", content.as_bytes(), &[], "other_path");
        assert_eq!(String::from_utf8_lossy(&result), "b");
    }

    #[test]
    fn change_file_stem_works() {
        let original_path = Path::new("example/file.txt");
        let new_stem = "new_file";
        let new_path = change_file_stem(original_path, new_stem);
        assert_eq!(new_path.to_string_lossy(), "example/new_file.txt");
    }

    #[test]
    fn fix_body_id_link_replaces_links_correctly() {
        let content = b"<html><body><a href='page1#id1'>Link</a></body></html>";
        let body_id_list = vec![("page1#id1".to_string(), "new_page1.xhtml".to_string())];
        let result = fix_body_id_link("file.xhtml", content, &body_id_list);
        assert_eq!(
            String::from_utf8_lossy(&result),
            "<html><body><a href='new_page1.xhtml'>Link</a></body></html>"
        );
    }

    #[test]
    fn fix_encoding_adds_xml_declaration() {
        let content = b"<html><body>Test</body></html>";
        let result = fix_encoding("file.xhtml", content);
        let expected = "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<html><body>Test</body></html>";
        assert_eq!(String::from_utf8_lossy(&result), expected);
    }

    #[test]
    fn fix_encoding_does_not_duplicate_xml_declaration() {
        let content = b"<?xml version=\"1.0\" encoding=\"utf-8\"?><html><body>Test</body></html>";
        let result = fix_encoding("file.xhtml", content);
        assert_eq!(
            String::from_utf8_lossy(&result),
            String::from_utf8_lossy(content)
        );
    }

    #[test]
    fn fix_stray_img_removes_stray_images() {
        let content = b"<html><body><img/><img src='valid.png'/></body></html>";
        let result = fix_stray_img("file.xhtml", content);

        let result_str = String::from_utf8_lossy(&result);

        let expected = "<html><head></head><body><img src=\"valid.png\"></body></html>";
        assert_eq!(
            result_str, expected,
            "Unexpected output structure after removing stray images."
        );
    }

    #[test]
    fn get_opf_filename_extracts_correct_path() {
        let content =
            b"<container><rootfiles><rootfile full-path='content.opf'/></rootfiles></container>";
        let result = get_opf_filename(content);
        assert_eq!(result, "content.opf");
    }

    #[test]
    fn fix_book_language_updates_language() {
        let content = b"<package xmlns=\"http://www.idpf.org/2007/opf\"><metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\"><dc:language>invalid</dc:language></metadata></package>";
        let opf_path = "content.opf".to_string();
        let result = fix_book_language("content.opf", content, &opf_path);
        assert!(String::from_utf8_lossy(&result).contains("<dc:language>en</dc:language>"));
    }

    #[test]
    fn fix_book_language_adds_language_tag() {
        let content = b"<package><metadata></metadata></package>";
        let opf_path = "content.opf".to_string();
        let result = fix_book_language("content.opf", content, &opf_path);
        assert!(String::from_utf8_lossy(&result).contains("<dc:language>en</dc:language>"));
    }

    #[test]
    fn simplify_language_works() {
        assert_eq!(simplify_language("en-US"), "en");
        assert_eq!(simplify_language("fr-CA"), "fr");
    }

    #[test]
    fn fix_language_updates_invalid_language() {
        let mut metadata = Element::new("metadata");
        let mut lang_tag = Element::new("language");
        lang_tag.children.push(XMLNode::Text("invalid".to_string()));
        metadata.children.push(XMLNode::Element(lang_tag));

        let changed = fix_language(&mut metadata);
        assert!(changed);
        assert_eq!(
            metadata.get_child("language").unwrap().get_text().unwrap(),
            "en"
        );
    }

    #[test]
    fn fix_language_does_not_change_valid_language() {
        let mut metadata = Element::new("metadata");
        let mut lang_tag = Element::new("language");
        lang_tag.children.push(XMLNode::Text("en".to_string()));
        metadata.children.push(XMLNode::Element(lang_tag));

        let changed = fix_language(&mut metadata);
        assert!(!changed);
        assert_eq!(
            metadata.get_child("language").unwrap().get_text().unwrap(),
            "en"
        );
    }
}
