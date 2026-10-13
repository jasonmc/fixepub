// XHTML 1's entity names constrain the broader HTML5 dependency table.
// Character values come from html-escape; XML's predefined entities are omitted.
const XHTML_NAMES: &str =
    "AElig Aacute Acirc Agrave Alpha Aring Atilde Auml Beta Ccedil Chi Dagger Delta ETH Eacute
Ecirc Egrave Epsilon Eta Euml Gamma Iacute Icirc Igrave Iota Iuml Kappa Lambda Mu Ntilde
Nu OElig Oacute Ocirc Ograve Omega Omicron Oslash Otilde Ouml Phi Pi Prime Psi Rho Scaron
Sigma THORN Tau Theta Uacute Ucirc Ugrave Upsilon Uuml Xi Yacute Yuml Zeta aacute acirc
acute aelig agrave alefsym alpha and ang aring asymp atilde auml bdquo beta brvbar bull
cap ccedil cedil cent chi circ clubs cong copy crarr cup curren dArr dagger darr deg delta
diams divide eacute ecirc egrave empty emsp ensp epsilon equiv eta eth euml euro exist
fnof forall frac12 frac14 frac34 frasl gamma ge hArr harr hearts hellip iacute icirc iexcl
igrave image infin int iota iquest isin iuml kappa lArr lambda lang laquo larr lceil ldquo
le lfloor lowast loz lrm lsaquo lsquo macr mdash micro middot minus mu nabla nbsp ndash ne
ni not notin nsub ntilde nu oacute ocirc oelig ograve oline omega omicron oplus or ordf
ordm oslash otilde otimes ouml para part permil perp phi pi piv plusmn pound prime prod
prop psi rArr radic rang raquo rarr rceil rdquo real reg rfloor rho rlm rsaquo rsquo sbquo
scaron sdot sect shy sigma sigmaf sim spades sub sube sum sup sup1 sup2 sup3 supe szlig
tau there4 theta thetasym thinsp thorn tilde times trade uArr uacute uarr ucirc ugrave uml
upsih upsilon uuml weierp xi yacute yen yuml zeta zwj zwnj";

pub(crate) fn entities() -> impl Iterator<Item = (&'static str, &'static str)> {
    XHTML_NAMES.split_ascii_whitespace().map(|name| {
        let table = &html_escape::NAMED_ENTITIES;
        let index = table
            .binary_search_by(|(key, _)| key.cmp(&name.as_bytes()))
            .expect("XHTML entity must exist in the HTML entity table");
        let value = match name {
            // HTML5 changed these two characters; retain XHTML 1's definitions.
            "lang" => "\u{2329}",
            "rang" => "\u{232a}",
            _ => table[index].1,
        };
        (name, value)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dependency_values_preserve_the_xhtml_entity_set() {
        let entries: std::collections::HashMap<_, _> = entities().collect();
        assert_eq!(entries.len(), 248);
        assert_eq!(entries["nbsp"], "\u{a0}");
        assert_eq!(entries["copy"], "©");
        assert_eq!(entries["lang"], "\u{2329}");
        assert_eq!(entries["rang"], "\u{232a}");
        assert!(!entries.contains_key("NotEqualTilde"));
    }
}
