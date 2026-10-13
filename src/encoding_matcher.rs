use nom::{
    error::{Error, ErrorKind},
    Err, IResult,
};

/// Recognize a complete XML declaration using the same parser as document repair.
pub fn is_xml_declaration(input: &str) -> IResult<&str, bool> {
    let invalid = || Err::Error(Error::new(input, ErrorKind::Tag));
    if !input.starts_with("<?xml") || !input.as_bytes().get(5).is_some_and(u8::is_ascii_whitespace)
    {
        return Err(invalid());
    }
    let end = input.find("?>").ok_or_else(invalid)? + 2;
    let document = format!("{}<root/>", &input[..end]);
    if xml::EventReader::from_str(&document)
        .into_iter()
        .any(|event| event.is_err())
    {
        return Err(invalid());
    }
    Ok((&input[end..], true))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_complete_declarations() {
        for declaration in [
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>",
            "<?xml version='1.0'?>",
            "<?xml version = '1.0' encoding = 'UTF-8' standalone='yes'?>",
        ] {
            let input = format!("{declaration}<html/>");
            assert_eq!(is_xml_declaration(&input), Ok(("<html/>", true)));
        }
    }

    #[test]
    fn rejects_malformed_declarations() {
        for input in [
            "<?xml encoding='utf-8'?>",
            "<?xml version='1.0' encoding='UTF-8'",
            "<?xml version='1.0\"?>",
            "<?xml-stylesheet href='style.css'?>",
        ] {
            assert!(is_xml_declaration(input).is_err(), "{input}");
        }
    }
}
