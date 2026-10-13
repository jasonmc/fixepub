use crate::error::FixError;
use indicatif::{ProgressBar, ProgressStyle};
use language_tags::LanguageTag;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};
use tempfile::NamedTempFile;
use xml::name::OwnedName;
use xml::reader::{ParserConfig, XmlEvent};
use xml::writer::EmitterConfig;
use zip::{write::SimpleFileOptions, CompressionMethod, ZipArchive, ZipWriter};

const XHTML: &str = "http://www.w3.org/1999/xhtml";
const DC: &str = "http://purl.org/dc/elements/1.1/";
const OPF: &str = "http://www.idpf.org/2007/opf";
const XML: &str = "http://www.w3.org/XML/1998/namespace";

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

fn xml_events<'a>(
    path: &'a str,
    content: impl Read + 'a,
) -> impl Iterator<Item = Result<XmlEvent, FixError>> + 'a {
    let mut reader = ParserConfig::new()
        .ignore_comments(false)
        .create_reader(content);
    let mut finished = false;
    let mut standalone = false;
    std::iter::from_fn(move || {
        if finished {
            return None;
        }
        let event = match reader.next() {
            Ok(event) => event,
            Err(error) => {
                finished = true;
                return Some(Err(xml_error(path, error)));
            }
        };
        match &event {
            XmlEvent::StartDocument {
                standalone: value, ..
            } => standalone = *value == Some(true),
            XmlEvent::Doctype { syntax } if is_xhtml(path) && !standalone => {
                // The XML parser gives configured entities precedence over the
                // internal subset. Supply only names not declared by the document.
                let declared = declared_entity_names(syntax);
                let entities =
                    crate::xhtml_entities::entities().filter(|(name, _)| !declared.contains(name));
                if let Err(error) = reader.add_entities(entities) {
                    finished = true;
                    return Some(Err(xml_error(path, error)));
                }
            }
            XmlEvent::EndDocument => finished = true,
            _ => {}
        }
        Some(Ok(event))
    })
}

fn declared_entity_names(mut syntax: &str) -> HashSet<&str> {
    let mut names = HashSet::new();
    while !syntax.is_empty() {
        if let Some(instruction) = syntax.strip_prefix("<?") {
            syntax = instruction
                .find("?>")
                .map_or("", |end| &instruction[end + 2..]);
            continue;
        }
        if let Some(comment) = syntax.strip_prefix("<!--") {
            syntax = comment.find("-->").map_or("", |end| &comment[end + 3..]);
            continue;
        }
        let first = syntax.chars().next().unwrap();
        if first == '\'' || first == '"' {
            let quoted = &syntax[first.len_utf8()..];
            syntax = quoted
                .find(first)
                .map_or("", |end| &quoted[end + first.len_utf8()..]);
            continue;
        }
        if let Some(declaration) = syntax.strip_prefix("<!ENTITY") {
            if declaration.starts_with(char::is_whitespace) {
                if let Some(name) = declaration.split_whitespace().next() {
                    // Parameter entities have separate names and cannot override
                    // these general character entity mappings directly.
                    if name != "%" {
                        names.insert(name);
                    }
                }
            }
        }
        syntax = &syntax[first.len_utf8()..];
    }
    names
}

fn xml_error(path: &str, error: impl std::fmt::Display) -> FixError {
    FixError::InvalidDocument {
        path: path.to_owned(),
        message: error.to_string(),
    }
}

fn parse(path: &str, content: &[u8]) -> Result<Vec<XmlEvent>, FixError> {
    xml_events(path, content).collect()
}

pub(crate) fn fix(
    filename: &Path,
    output_filename: &Path,
    language: &str,
    force: bool,
) -> Result<(), FixError> {
    if output_filename.exists() && !force {
        return Err(FixError::OutputExists(output_filename.to_path_buf()));
    }
    let mut archive = ZipArchive::new(File::open(filename)?)?;
    let mut names = Vec::with_capacity(archive.len());
    let mut unique = HashSet::new();
    for i in 0..archive.len() {
        let name = archive.by_index(i)?.name()?.into_owned();
        if !unique.insert(name.clone()) {
            return Err(xml_error(&name, "duplicate archive entry"));
        }
        names.push(name);
    }

    let mut container = Vec::new();
    archive
        .by_name("META-INF/container.xml")?
        .read_to_end(&mut container)?;
    let opf_path = get_opf_filename(&container)?;
    if !unique.contains(&opf_path) {
        return Err(xml_error(&opf_path, "package document is missing"));
    }

    // Only read as far as the body start tag in this pass. Images, fonts, and
    // other binary assets are never decompressed or retained in memory.
    let mut body_ids = HashMap::new();
    for name in names.iter().filter(|name| is_xhtml(name)) {
        if let Some(id) = collect_body_id(name, BufReader::new(archive.by_name(name)?))? {
            body_ids.insert(name.clone(), id);
        }
    }

    let parent = output_filename
        .parent()
        .filter(|p| !p.as_os_str().is_empty());
    let mut temporary = NamedTempFile::new_in(parent.unwrap_or(Path::new(".")))?;
    {
        let mut output = ZipWriter::new(temporary.as_file_mut());
        output.set_raw_comment(archive.comment().into())?;
        // EPUB's mimetype entry must be first, stored, and have no extra fields.
        output.start_file(
            "mimetype",
            SimpleFileOptions::default().compression_method(CompressionMethod::Stored),
        )?;
        output.write_all(b"application/epub+zip")?;
        let pb = ProgressBar::new(names.len() as u64);
        pb.set_style(ProgressStyle::with_template(
            "{spinner:.green} [{wide_bar:.cyan/blue}] {pos}/{len}",
        )?);
        for name in names {
            if name != "mimetype" {
                if is_xhtml(&name) || name == opf_path {
                    let mut file = archive.by_name(&name)?;
                    let options = file.options();
                    let mut content = Vec::new();
                    file.read_to_end(&mut content)?;
                    drop(file);
                    let repaired =
                        process_document(&name, &content, &body_ids, name == opf_path, language)?;
                    if let Some(repaired) = repaired {
                        output.start_file(&name, options)?;
                        output.write_all(&repaired)?;
                    } else {
                        output.raw_copy_file(archive.by_name(&name)?)?;
                    }
                } else {
                    output.raw_copy_file(archive.by_name(&name)?)?;
                }
            }
            pb.inc(1);
        }
        output.finish()?;
        pb.finish_and_clear();
    }
    temporary.as_file().sync_all()?;
    if force {
        temporary
            .persist(output_filename)
            .map_err(|error| error.error)?;
    } else {
        temporary
            .persist_noclobber(output_filename)
            .map_err(|error| {
                if error.error.kind() == std::io::ErrorKind::AlreadyExists {
                    FixError::OutputExists(output_filename.to_path_buf())
                } else {
                    FixError::Io(error.error)
                }
            })?;
    }
    Ok(())
}

fn is_xhtml(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("html") || extension.eq_ignore_ascii_case("xhtml")
        })
}

fn is_element(name: &OwnedName, local: &str, namespace: &str) -> bool {
    name.local_name == local && name.namespace.as_deref().is_none_or(|ns| ns == namespace)
}

fn collect_body_id(path: &str, content: impl Read) -> Result<Option<String>, FixError> {
    for event in xml_events(path, content) {
        if let XmlEvent::StartElement {
            name, attributes, ..
        } = event?
        {
            if is_element(&name, "body", XHTML) {
                return Ok(attributes
                    .into_iter()
                    .find(|attribute| {
                        attribute.name.local_name == "id" && attribute.name.namespace.is_none()
                    })
                    .map(|attribute| attribute.value)
                    .filter(|value| !value.is_empty()));
            }
        }
    }
    Ok(None)
}

fn get_opf_filename(content: &[u8]) -> Result<String, FixError> {
    let events = parse("META-INF/container.xml", content)?;
    let mut candidates = Vec::new();
    let mut in_rootfiles = false;
    for event in events {
        match event {
            XmlEvent::StartElement { name, .. } if name.local_name == "rootfiles" => {
                in_rootfiles = true
            }
            XmlEvent::EndElement { name } if name.local_name == "rootfiles" => in_rootfiles = false,
            XmlEvent::StartElement {
                name, attributes, ..
            } if in_rootfiles && name.local_name == "rootfile" => {
                let path = attributes.iter().find(|a| a.name.local_name == "full-path");
                let media_type = attributes
                    .iter()
                    .find(|a| a.name.local_name == "media-type");
                if let Some(path) = path {
                    candidates.push((
                        path.value.clone(),
                        media_type.is_some_and(|a| a.value == "application/oebps-package+xml"),
                    ));
                }
            }
            _ => {}
        }
    }
    candidates.sort_by_key(|(_, preferred)| !preferred);
    candidates
        .into_iter()
        .next()
        .map(|(path, _)| path)
        .ok_or_else(|| xml_error("META-INF/container.xml", "no package document was declared"))
}

fn process_document(
    path: &str,
    content: &[u8],
    body_ids: &HashMap<String, String>,
    is_package: bool,
    language: &str,
) -> Result<Option<Vec<u8>>, FixError> {
    let mut events = parse(path, content)?;
    let mut changed = if is_package {
        repair_language(path, &mut events, language)?
    } else {
        repair_xhtml(path, &mut events, body_ids)
    };
    // The parser decodes supported encodings without replacement characters.
    // Any serialized document is explicitly emitted as UTF-8.
    if let Some(XmlEvent::StartDocument { encoding, .. }) = events.first_mut() {
        changed |= !encoding.eq_ignore_ascii_case("utf-8");
        *encoding = "UTF-8".to_owned();
    }
    if !is_package {
        changed |= !content
            .strip_prefix(b"\xef\xbb\xbf")
            .unwrap_or(content)
            .starts_with(b"<?xml ");
    }
    if !changed {
        return Ok(None);
    }
    let mut result = Vec::new();
    {
        let mut writer = EmitterConfig::new()
            .autopad_comments(false)
            .create_writer(&mut result);
        for event in &events {
            if let Some(event) = event.as_writer_event() {
                writer
                    .write(event)
                    .map_err(|error| xml_error(path, error))?;
            }
        }
    }
    Ok(Some(result))
}

fn repair_xhtml(path: &str, events: &mut Vec<XmlEvent>, bodies: &HashMap<String, String>) -> bool {
    let mut changed = false;
    // ZIP entry names are decoded filenames, not URI references. Escape them
    // once at this boundary so literal %, #, and ? survive URL resolution.
    let archive_base = encode_archive_path(path);
    let document_base = events
        .iter()
        .find_map(|event| {
            if let XmlEvent::StartElement {
                name, attributes, ..
            } = event
            {
                if is_element(name, "base", XHTML) {
                    return attributes
                        .iter()
                        .find(|a| a.name.local_name == "href" && a.name.namespace.is_none())
                        .map(|a| {
                            resolve_local(&archive_base, &a.value)
                                .unwrap_or_else(|| format!("external:{}", a.value))
                        });
                }
            }
            None
        })
        .unwrap_or(archive_base);
    let mut bases = vec![document_base];
    let mut skip_depth = 0;
    events.retain_mut(|event| {
        match event {
            XmlEvent::StartElement {
                name, attributes, ..
            } => {
                if skip_depth > 0 {
                    skip_depth += 1;
                    return false;
                }
                if is_element(name, "img", XHTML)
                    && !attributes.iter().any(|a| {
                        a.name.local_name == "src"
                            && a.name.namespace.is_none()
                            && !a.value.trim().is_empty()
                    })
                {
                    skip_depth = 1;
                    changed = true;
                    return false;
                }
                let parent_base = bases.last().unwrap();
                let base = attributes
                    .iter()
                    .find(|a| {
                        a.name.local_name == "base" && a.name.namespace.as_deref() == Some(XML)
                    })
                    .map(|a| {
                        resolve_local(parent_base, &a.value)
                            .unwrap_or_else(|| format!("external:{}", a.value))
                    })
                    .unwrap_or_else(|| parent_base.clone());
                if is_element(name, "a", XHTML) || is_element(name, "area", XHTML) {
                    for attribute in attributes
                        .iter_mut()
                        .filter(|a| a.name.local_name == "href" && a.name.namespace.is_none())
                    {
                        if let Some(target) = repaired_link(&base, &attribute.value, bodies) {
                            attribute.value = target;
                            changed = true;
                        }
                    }
                }
                bases.push(base);
            }
            XmlEvent::EndElement { .. } => {
                if skip_depth > 0 {
                    skip_depth -= 1;
                    return false;
                }
                bases.pop();
            }
            _ if skip_depth > 0 => return false,
            _ => {}
        }
        true
    });
    changed
}

fn encode_archive_path(path: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut encoded = String::with_capacity(path.len());
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'.' | b'_' | b'~') {
            encoded.push(byte as char);
        } else {
            encoded.push('%');
            encoded.push(HEX[(byte >> 4) as usize] as char);
            encoded.push(HEX[(byte & 15) as usize] as char);
        }
    }
    encoded
}

fn decode_uri(value: &str) -> Option<String> {
    let mut decoded = Vec::with_capacity(value.len());
    let mut bytes = value.bytes();
    while let Some(byte) = bytes.next() {
        if byte == b'%' {
            let high = (bytes.next()? as char).to_digit(16)?;
            let low = (bytes.next()? as char).to_digit(16)?;
            decoded.push((high * 16 + low) as u8);
        } else {
            decoded.push(byte);
        }
    }
    String::from_utf8(decoded).ok()
}

// Both arguments are URI paths/references; archive filenames must be escaped first.
fn resolve_local(base: &str, reference: &str) -> Option<String> {
    let base = base.split(['?', '#']).next()?;
    let reference = reference.split(['?', '#']).next()?;
    // A query value can contain a colon without making the URL external.
    // A scheme can only occur before the first slash in the path portion.
    let has_scheme = |path: &str| {
        path.split('/')
            .next()
            .is_some_and(|part| part.contains(':'))
    };
    if has_scheme(base) || has_scheme(reference) || reference.starts_with('/') {
        return None;
    }
    if reference.is_empty() {
        return Some(base.split(['?', '#']).next()?.to_owned());
    }
    let parent = base.rsplit_once('/').map_or("", |(parent, _)| parent);
    let joined = if parent.is_empty() {
        reference.to_owned()
    } else {
        format!("{parent}/{reference}")
    };
    let mut components = Vec::new();
    for component in joined.split('/') {
        match component {
            "" | "." => {}
            ".." => {
                components.pop()?;
            }
            component => components.push(component),
        }
    }
    let mut result = components.join("/");
    let directory =
        reference.ends_with('/') || matches!(reference.rsplit('/').next(), Some("." | ".."));
    if directory && !result.is_empty() {
        result.push('/');
    }
    Some(result)
}

fn repaired_link(base: &str, href: &str, bodies: &HashMap<String, String>) -> Option<String> {
    let (reference, fragment) = href.split_once('#')?;
    let target = decode_uri(&resolve_local(base, reference)?)?;
    if bodies.get(&target)? != &decode_uri(fragment)? {
        return None;
    }
    // An empty href has inconsistent reader behavior; use an explicit self URL.
    if reference.is_empty() {
        Some(base.rsplit('/').next()?.to_owned())
    } else {
        Some(reference.to_owned())
    }
}

fn element_end(events: &[XmlEvent], start: usize) -> usize {
    let mut depth = 0;
    for (offset, event) in events[start..].iter().enumerate() {
        match event {
            XmlEvent::StartElement { .. } => depth += 1,
            XmlEvent::EndElement { .. } => {
                depth -= 1;
                if depth == 0 {
                    return start + offset;
                }
            }
            _ => {}
        }
    }
    unreachable!("parsed XML has balanced elements")
}

fn valid_language(value: &str) -> bool {
    LanguageTag::parse(value).is_ok_and(|tag| tag.validate().is_ok())
}

fn repair_language(
    path: &str,
    events: &mut Vec<XmlEvent>,
    fallback: &str,
) -> Result<bool, FixError> {
    let metadata_start = events
        .iter()
        .position(|event| {
            matches!(event,
                XmlEvent::StartElement { name, .. } if is_element(name, "metadata", OPF)
            )
        })
        .ok_or_else(|| xml_error(path, "package metadata is missing"))?;
    let metadata_end = element_end(events, metadata_start);
    let mut replacements = Vec::new();
    let mut found = false;
    let mut i = metadata_start + 1;
    while i < metadata_end {
        if let XmlEvent::StartElement { name, .. } = &events[i] {
            let end = element_end(events, i);
            if is_element(name, "language", DC) {
                found = true;
                let value: String = events[i + 1..end]
                    .iter()
                    .filter_map(|event| match event {
                        XmlEvent::Characters(text)
                        | XmlEvent::Whitespace(text)
                        | XmlEvent::CData(text) => Some(text.as_str()),
                        _ => None,
                    })
                    .collect();
                if !valid_language(&value) {
                    let replacement = if valid_language(value.trim()) {
                        value.trim()
                    } else {
                        fallback
                    };
                    replacements.push((
                        i + 1,
                        end,
                        vec![XmlEvent::Characters(replacement.to_owned())],
                    ));
                }
            }
            i = end + 1;
        } else {
            i += 1;
        }
    }
    if !found {
        let mut namespace = xml::namespace::Namespace::empty();
        namespace.put("dc", DC);
        let name = OwnedName {
            local_name: "language".to_owned(),
            namespace: Some(DC.to_owned()),
            prefix: Some("dc".to_owned()),
        };
        replacements.push((
            metadata_end,
            metadata_end,
            vec![
                XmlEvent::StartElement {
                    name: name.clone(),
                    attributes: vec![],
                    namespace,
                },
                XmlEvent::Characters(fallback.to_owned()),
                XmlEvent::EndElement { name },
            ],
        ));
    }
    let changed = !replacements.is_empty();
    for (start, end, replacement) in replacements.into_iter().rev() {
        events.splice(start..end, replacement);
    }
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repair(path: &str, text: &str, bodies: &[(&str, &str)]) -> Vec<u8> {
        let bodies = bodies
            .iter()
            .map(|(path, id)| (path.to_string(), id.to_string()))
            .collect();
        let result = process_document(path, text.as_bytes(), &bodies, false, "en")
            .unwrap()
            .unwrap_or_else(|| text.as_bytes().to_vec());
        parse(path, &result).expect("repaired XHTML must be valid XML");
        result
    }

    fn hrefs(content: &[u8]) -> Vec<String> {
        parse("test.xhtml", content)
            .unwrap()
            .into_iter()
            .filter_map(|event| {
                if let XmlEvent::StartElement { attributes, .. } = event {
                    attributes
                        .into_iter()
                        .find(|a| a.name.local_name == "href")
                        .map(|a| a.value)
                } else {
                    None
                }
            })
            .collect()
    }

    #[test]
    fn image_cleanup_preserves_xml_namespaces_comments_and_cdata() {
        let input = format!(
            r#"<html xmlns="{XHTML}" xmlns:epub="http://www.idpf.org/2007/ops"><body><img/><!--keep--><img src="cover.png"/><span epub:type="pagebreak"/><script><![CDATA[a < b]]></script></body></html>"#
        );
        let output = repair("nav.xhtml", &input, &[]);
        let events = parse("nav.xhtml", &output).unwrap();
        assert_eq!(events.iter().filter(|event| matches!(event, XmlEvent::StartElement { name, .. } if name.local_name == "img")).count(), 1);
        assert!(events
            .iter()
            .any(|event| matches!(event, XmlEvent::Comment(text) if text == "keep")));
        assert!(events
            .iter()
            .any(|event| matches!(event, XmlEvent::CData(text) if text == "a < b")));
        assert!(events.iter().any(|event| matches!(event, XmlEvent::StartElement { attributes, .. } if attributes.iter().any(|a| a.name.namespace.as_deref() == Some("http://www.idpf.org/2007/ops") && a.value == "pagebreak"))));
    }

    #[test]
    fn links_use_full_paths_and_only_change_matching_attributes() {
        let output = repair(
            "nav/nav.xhtml",
            r#"<html><body><a href="../a/chapter.xhtml#one">chapter.xhtml#one</a><a href="../b/chapter.xhtml#one"/><a href="https://example.com/a/chapter.xhtml#one"/><a href="../a/chapter.xhtml#one-extra"/><p title="../a/chapter.xhtml#one"/></body></html>"#,
            &[("a/chapter.xhtml", "one"), ("b/chapter.xhtml", "two")],
        );
        assert_eq!(
            hrefs(&output),
            [
                "../a/chapter.xhtml",
                "../b/chapter.xhtml#one",
                "https://example.com/a/chapter.xhtml#one",
                "../a/chapter.xhtml#one-extra"
            ]
        );
        let output = String::from_utf8(output).unwrap();
        assert!(output.contains(">chapter.xhtml#one</a>"));
        assert!(output.contains("title=\"../a/chapter.xhtml#one\""));
    }

    #[test]
    fn links_support_relative_self_encoded_paths_queries_and_xml_base() {
        let output = repair(
            "text/nav.xhtml",
            r##"<html><body><a href="#nav"/><a href="./ch%20one.xhtml?x=1#b%20id"/><div xml:base="../chapters/"><a href="chapter.xhtml#chapter"/></div><div xml:base="https://example.com/"><div xml:base="relative/"><a href="chapter.xhtml#chapter"/></div></div></body></html>"##,
            &[
                ("text/nav.xhtml", "nav"),
                ("text/ch one.xhtml", "b id"),
                ("chapters/chapter.xhtml", "chapter"),
            ],
        );
        assert_eq!(
            hrefs(&output),
            [
                "nav.xhtml",
                "./ch%20one.xhtml?x=1",
                "chapter.xhtml",
                "chapter.xhtml#chapter"
            ]
        );
    }

    #[test]
    fn unchanged_documents_are_byte_preserved() {
        let content = b"<?xml version='1.0'?><html><body><img src='cover.png'/></body></html>";
        assert!(
            process_document("chapter.xhtml", content, &HashMap::new(), false, "en")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn repairs_are_idempotent() {
        let output = repair(
            "nav.xhtml",
            "<html><body><img/><a href='chapter.xhtml#body'/></body></html>",
            &[("chapter.xhtml", "body")],
        );
        let bodies = HashMap::from([("chapter.xhtml".to_string(), "body".to_string())]);
        assert!(process_document("nav.xhtml", &output, &bodies, false, "en")
            .unwrap()
            .is_none());
    }

    #[test]
    fn invalid_encoding_or_xml_is_an_error() {
        for content in [
            b"<html><body>\xff</body></html>".as_slice(),
            b"<?xml version='1.0' encoding='bogus'?><html/>",
            b"<html><body><img></body></html>",
        ] {
            assert!(
                process_document("chapter.xhtml", content, &HashMap::new(), false, "en").is_err()
            );
        }
    }

    #[test]
    fn utf16_is_transcoded_without_losing_characters() {
        let text = "<?xml version='1.0' encoding='UTF-16'?><html><body>café 日本語</body></html>";
        let mut content = vec![0xff, 0xfe];
        content.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
        let output = process_document("chapter.xhtml", &content, &HashMap::new(), false, "en")
            .unwrap()
            .unwrap();
        assert!(String::from_utf8(output.clone())
            .unwrap()
            .contains("café 日本語"));
        parse("chapter.xhtml", &output).unwrap();
    }

    #[test]
    fn missing_language_gets_a_bound_namespace_and_selected_fallback() {
        let input = format!(r#"<package xmlns="{OPF}"><metadata/></package>"#);
        let output = process_document("content.opf", input.as_bytes(), &HashMap::new(), true, "fr")
            .unwrap()
            .unwrap();
        let events = parse("content.opf", &output).unwrap();
        assert!(events.iter().any(|event| matches!(event, XmlEvent::StartElement { name, .. } if name.local_name == "language" && name.namespace.as_deref() == Some(DC))));
        assert!(events
            .iter()
            .any(|event| matches!(event, XmlEvent::Characters(text) if text == "fr")));
        assert!(
            process_document("content.opf", &output, &HashMap::new(), true, "fr")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn language_repairs_preserve_other_namespaced_metadata() {
        let input = format!(
            r#"<package xmlns="{OPF}" xmlns:dc="{DC}"><metadata><dc:language> fr-CA </dc:language><dc:language>invalid</dc:language><dc:title xml:lang="fr">Titre</dc:title></metadata></package>"#
        );
        let output = process_document("content.opf", input.as_bytes(), &HashMap::new(), true, "de")
            .unwrap()
            .unwrap();
        let output = String::from_utf8(output).unwrap();
        assert!(output.contains(">fr-CA</dc:language>"));
        assert!(output.contains(">de</dc:language>"));
        assert!(output.contains("xml:lang=\"fr\""));
    }

    #[test]
    fn package_path_prefers_the_opf_media_type() {
        let input = br#"<container><rootfiles><rootfile full-path="other.xml"/><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#;
        assert_eq!(get_opf_filename(input).unwrap(), "OEBPS/content.opf");
        assert!(get_opf_filename(b"<container/>").is_err());
        assert!(get_opf_filename(b"<container>").is_err());
    }

    #[test]
    fn recognizes_uppercase_document_extensions() {
        assert!(is_xhtml("text/CHAPTER.XHTML"));
        assert!(is_xhtml("text/CHAPTER.HTML"));
        assert!(!is_xhtml("image.png"));
    }

    #[test]
    fn links_respect_the_document_base_url() {
        let output = repair(
            "nav.xhtml",
            r#"<html><head><base href="chapters/"/></head><body><a href="chapter.xhtml#body"/></body></html>"#,
            &[("chapters/chapter.xhtml", "body")],
        );
        assert_eq!(hrefs(&output), ["chapters/", "chapter.xhtml"]);
        let output = repair(
            "nav.xhtml",
            r#"<html><head><base href="https://example.com/"/></head><body><a href="chapter.xhtml#body"/></body></html>"#,
            &[("chapter.xhtml", "body")],
        );
        assert_eq!(
            hrefs(&output),
            ["https://example.com/", "chapter.xhtml#body"]
        );
    }
    #[test]
    fn xhtml_dtd_entities_are_preserved_through_repairs() {
        let input = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.1//EN" "http://www.w3.org/TR/xhtml11/DTD/xhtml11.dtd">
<html xmlns="http://www.w3.org/1999/xhtml"><head><title>&copy; &Omega;</title></head><body id="b&alpha;"><p>A&nbsp;B &mdash; &euro;</p><img/></body></html>"#;
        assert_eq!(
            collect_body_id("chapter.xhtml", input.as_bytes())
                .unwrap()
                .as_deref(),
            Some("bα")
        );
        let output = repair("chapter.xhtml", input, &[]);
        let text = String::from_utf8(output).unwrap();
        assert!(text.contains("© Ω"));
        assert!(text.contains("A\u{a0}B — €"));
        assert!(!text.contains("<img"));
    }

    #[test]
    fn unknown_entities_remain_errors() {
        assert!(parse(
            "chapter.xhtml",
            b"<html><body>&notAnXhtmlEntity;</body></html>"
        )
        .is_err());
        assert!(parse("content.opf", b"<package>&nbsp;</package>").is_err());
    }

    #[test]
    fn local_links_with_colons_in_queries_are_repaired() {
        let bodies = HashMap::from([("chapter.xhtml".to_owned(), "body".to_owned())]);
        assert_eq!(
            repaired_link("nav.xhtml", "chapter.xhtml?value=urn:abc#body", &bodies).as_deref(),
            Some("chapter.xhtml?value=urn:abc")
        );
        assert_eq!(
            repaired_link("nav.xhtml?value=urn:abc", "chapter.xhtml#body", &bodies).as_deref(),
            Some("chapter.xhtml")
        );
        assert!(repaired_link(
            "nav.xhtml",
            "https://example.com/chapter.xhtml?value=urn:abc#body",
            &bodies
        )
        .is_none());
        assert!(repaired_link(
            "https://example.com/nav.xhtml",
            "chapter.xhtml?value=urn:abc#body",
            &bodies
        )
        .is_none());
        assert_eq!(
            resolve_local("text/nav.xhtml", "../text/dir:one/chapter.xhtml").as_deref(),
            Some("text/dir:one/chapter.xhtml")
        );
    }
    #[test]
    fn document_entity_declarations_take_precedence_over_defaults() {
        let input = r#"<!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.1//EN" "http://www.w3.org/TR/xhtml11/DTD/xhtml11.dtd" [<!ENTITY nbsp "CUSTOM">]><html><body id="b&nbsp;"><p>A&nbsp;B &copy;</p><img/></body></html>"#;
        assert_eq!(
            collect_body_id("chapter.xhtml", input.as_bytes())
                .unwrap()
                .as_deref(),
            Some("bCUSTOM")
        );
        let output = String::from_utf8(repair("chapter.xhtml", input, &[])).unwrap();
        assert!(output.contains("ACUSTOMB ©"));
        assert!(output.contains("id=\"bCUSTOM\""));
    }
    #[test]
    fn commented_and_quoted_entity_examples_do_not_override_defaults() {
        let input = r#"<!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.1//EN" "http://www.w3.org/TR/xhtml11/DTD/xhtml11.dtd" [<!-- Example: <!ENTITY nbsp "CUSTOM"> --><?example <!ENTITY euro "CUSTOM"> ?><!ENTITY example '<!ENTITY copy "CUSTOM">'>]><html><body><p>A&nbsp;B &copy; &euro;</p><img/></body></html>"#;
        let output = String::from_utf8(repair("chapter.xhtml", input, &[])).unwrap();
        assert!(output.contains("A\u{a0}B © €"));
        assert_eq!(
            declared_entity_names(
                r#"<!-- <!ENTITY nbsp "CUSTOM"> --><!ENTITY example '<!ENTITY copy "CUSTOM">'>"#
            ),
            HashSet::from(["example"])
        );
    }
    #[test]
    fn html5_only_entities_are_not_added_to_xhtml_dtds() {
        let input = br#"<!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.1//EN" "http://www.w3.org/TR/xhtml11/DTD/xhtml11.dtd"><html><body>&NotEqualTilde;</body></html>"#;
        assert!(parse("chapter.xhtml", input).is_err());
    }
    #[test]
    fn dot_directory_bases_keep_their_directory_semantics() {
        for base in ["chapters/.", "chapters/../chapters/.", "chapters/deep/.."] {
            let input = format!(
                r#"<html><body xml:base="{base}"><a href="chapter.xhtml#target"/><a href="chapter.xhtml#other"/></body></html>"#
            );
            let output = repair(
                "OEBPS/nav.xhtml",
                &input,
                &[
                    ("OEBPS/chapters/chapter.xhtml", "target"),
                    ("OEBPS/chapter.xhtml", "other"),
                ],
            );
            assert_eq!(
                hrefs(&output),
                ["chapter.xhtml", "chapter.xhtml#other"],
                "{base}"
            );
        }
        assert_eq!(resolve_local("nav.xhtml", ".").as_deref(), Some(""));
        let output = repair(
            "nav.xhtml",
            r#"<html><body xml:base="."><a href="chapter.xhtml#body"/></body></html>"#,
            &[("chapter.xhtml", "body")],
        );
        assert_eq!(hrefs(&output), ["chapter.xhtml"]);
    }

    #[test]
    fn archive_path_characters_are_not_uri_delimiters_or_escape_sequences() {
        for directory in [
            "Part#1",
            "Part?1",
            "Part%1",
            "Part%23",
            "Part%2F",
            "Part:1",
            "Part café",
        ] {
            let nav = format!("{directory}/nav#?%.xhtml");
            let chapter = format!("{directory}/chapter.xhtml");
            let output = repair(
                &nav,
                r##"<html><body id="nav"><a href="chapter.xhtml#body"/><a href="#nav"/></body></html>"##,
                &[(&chapter, "body"), (&nav, "nav")],
            );
            assert_eq!(
                hrefs(&output),
                ["chapter.xhtml", "nav%23%3F%25.xhtml"],
                "{directory}"
            );
        }
    }
    #[test]
    fn change_file_stem_works() {
        let original_path = Path::new("example/file.txt");
        let new_stem = "new_file";
        let new_path = change_file_stem(original_path, new_stem);
        assert_eq!(new_path.to_string_lossy(), "example/new_file.txt");
    }
}
