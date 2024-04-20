use nom::{
    branch::alt,
    bytes::complete::{tag_no_case, take_while},
    character::complete::{char, multispace1},
    combinator::{opt, map},
    sequence::{delimited, preceded, tuple},
    IResult,
};

pub fn is_xml_declaration(input: &str) -> IResult<&str, bool> {
    let version_parser = preceded(
        tag_no_case("version="),
        delimited(
            alt((char('\''), char('"'))),
            take_while(|c: char| c.is_ascii_digit() || c == '.'),
            alt((char('\''), char('"'))),
        ),
    );

    let encoding_parser = preceded(
        tag_no_case("encoding="),
        delimited(
            alt((char('\''), char('"'))),
            take_while(|c: char| c.is_ascii_alphanumeric() || c == '-' || c == '.'),
            alt((char('\''), char('"'))),
        ),
    );

    let xml_declaration_parser = delimited(
        tag_no_case("<?xml"),
        tuple((
            preceded(multispace1, version_parser),
            preceded(multispace1, encoding_parser),
        )),
        opt(preceded(
            take_while(|c: char| c != '?'),
            tag_no_case("?>"),
        )),
    );

    map(
        xml_declaration_parser,
        |_| true // Map the result to `true` to indicate successful parsing
    )(input)
}