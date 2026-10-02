//! Excel's XML Spreadsheet 2003: a workbook written as one XML document.
//!
//! iShares hands its fund downloads out in this format under an `.xls` name.
//! It is text on disk, so the container sniff calls it text and calamine
//! cannot open it. The structure is small -- worksheets of rows of cells of
//! data -- and this reads exactly that, as strings, leaving every cell's
//! meaning to the parser that asked.

use quick_xml::events::Event;
use quick_xml::Reader;

use crate::decode::DecodeError;

/// The namespace every XML Spreadsheet 2003 document declares.
pub const NAMESPACE: &str = "urn:schemas-microsoft-com:office:spreadsheet";

/// One worksheet: its name, and its rows as cell text.
#[derive(Debug, Clone)]
pub struct XmlSheet {
    pub name: String,
    pub rows: Vec<Vec<String>>,
}

/// A workbook read once into owned sheets.
#[derive(Debug, Clone)]
pub struct XmlSheets {
    pub sheets: Vec<XmlSheet>,
}

impl XmlSheets {
    pub fn open(text: &str) -> Result<Self, DecodeError> {
        // Some downloads repeat the byte-order mark, so strip every one.
        let text = text.trim_start_matches('\u{FEFF}');
        if !text.contains(NAMESPACE) {
            return Err(DecodeError::NotThisContainer(
                "Not an XML Spreadsheet 2003 document".to_string(),
            ));
        }

        let text = escape_bare_ampersands(text);
        let mut reader = Reader::from_str(&text);
        let mut sheets = Vec::new();
        let mut sheet: Option<XmlSheet> = None;
        let mut row: Option<Vec<String>> = None;
        // The cell being read, and whether we are inside its <Data>.
        let mut cell: Option<String> = None;
        let mut in_data = false;

        loop {
            let event = reader
                .read_event()
                .map_err(|e| DecodeError::Damaged(format!("XML Spreadsheet: {}", e)))?;
            match event {
                Event::Start(e) => match e.local_name().as_ref() {
                    b"Worksheet" => {
                        let name = attribute(&e, b"Name").unwrap_or_default();
                        sheet = Some(XmlSheet {
                            name,
                            rows: Vec::new(),
                        });
                    }
                    b"Row" => row = Some(Vec::new()),
                    b"Cell" => {
                        pad_to_index(&mut row, &e);
                        cell = Some(String::new());
                    }
                    b"Data" => in_data = true,
                    _ => {}
                },
                Event::Empty(e) => match e.local_name().as_ref() {
                    // A self-closing row or cell is an empty one, and still
                    // takes its place.
                    b"Row" => {
                        if let Some(s) = sheet.as_mut() {
                            s.rows.push(Vec::new());
                        }
                    }
                    b"Cell" => {
                        pad_to_index(&mut row, &e);
                        if let Some(r) = row.as_mut() {
                            r.push(String::new());
                        }
                    }
                    _ => {}
                },
                Event::Text(t) if in_data => {
                    if let (Some(c), Ok(text)) = (cell.as_mut(), t.xml10_content()) {
                        c.push_str(&text);
                    }
                }
                Event::CData(t) if in_data => {
                    if let (Some(c), Ok(text)) = (cell.as_mut(), t.decode()) {
                        c.push_str(&text);
                    }
                }
                Event::GeneralRef(r) if in_data => {
                    if let Some(c) = cell.as_mut() {
                        if let Ok(Some(ch)) = r.resolve_char_ref() {
                            c.push(ch);
                        } else if let Ok(name) = r.decode() {
                            let raw = format!("&{};", name);
                            match quick_xml::escape::unescape(&raw) {
                                Ok(text) => c.push_str(&text),
                                Err(_) => c.push_str(&raw),
                            }
                        }
                    }
                }
                Event::End(e) => match e.local_name().as_ref() {
                    b"Data" => in_data = false,
                    b"Cell" => {
                        if let (Some(r), Some(c)) = (row.as_mut(), cell.take()) {
                            r.push(c.trim().to_string());
                        }
                    }
                    b"Row" => {
                        if let (Some(s), Some(r)) = (sheet.as_mut(), row.take()) {
                            s.rows.push(r);
                        }
                    }
                    b"Worksheet" => {
                        if let Some(s) = sheet.take() {
                            sheets.push(s);
                        }
                    }
                    _ => {}
                },
                Event::Eof => break,
                _ => {}
            }
        }

        if sheets.is_empty() {
            return Err(DecodeError::NotThisContainer(
                "XML Spreadsheet has no worksheets".to_string(),
            ));
        }
        Ok(XmlSheets { sheets })
    }

    pub fn sheet(&self, name: &str) -> Option<&XmlSheet> {
        self.sheets.iter().find(|s| s.name == name)
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.sheets.iter().map(|s| s.name.as_str())
    }
}

/// Escapes every `&` that does not begin an entity.
///
/// iShares writes links into its workbooks with their query strings as they
/// are -- `…#!type=ishares&style=All` -- which is not well-formed XML, and a
/// strict reader stops there. Excel opens the file anyway; so must this.
fn escape_bare_ampersands(text: &str) -> std::borrow::Cow<'_, str> {
    fn starts_entity(rest: &str) -> bool {
        let body = rest.split(';').next().unwrap_or("");
        if body.len() == rest.len() || body.is_empty() || body.len() > 10 {
            return false;
        }
        match body.strip_prefix('#') {
            Some(num) => match num.strip_prefix('x') {
                Some(hex) => !hex.is_empty() && hex.chars().all(|c| c.is_ascii_hexdigit()),
                None => !num.is_empty() && num.chars().all(|c| c.is_ascii_digit()),
            },
            None => body.chars().all(|c| c.is_ascii_alphanumeric()),
        }
    }
    if !text
        .match_indices('&')
        .any(|(i, _)| !starts_entity(&text[i + 1..]))
    {
        return std::borrow::Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len() + 64);
    for (i, ch) in text.char_indices() {
        if ch == '&' && !starts_entity(&text[i + 1..]) {
            out.push_str("&amp;");
        } else {
            out.push(ch);
        }
    }
    std::borrow::Cow::Owned(out)
}

/// The unprefixed value of an attribute, matched on its local name so the
/// `ss:` prefix a document chooses does not matter.
fn attribute(e: &quick_xml::events::BytesStart<'_>, local: &[u8]) -> Option<String> {
    // Decoded by hand rather than through quick-xml's attribute helpers,
    // whose names change with the crate's feature set: calamine turns on its
    // `encoding` feature, which removes the plain one.
    e.attributes().flatten().find_map(|a| {
        if a.key.local_name().as_ref() != local {
            return None;
        }
        let raw = std::str::from_utf8(&a.value).ok()?;
        Some(
            quick_xml::escape::unescape(raw)
                .map(|v| v.into_owned())
                .unwrap_or_else(|_| raw.to_string()),
        )
    })
}

/// A cell may name its 1-based column with `ss:Index`, skipping the empty
/// ones before it. Pads the row so the cell lands in that column.
fn pad_to_index(row: &mut Option<Vec<String>>, e: &quick_xml::events::BytesStart<'_>) {
    let (Some(r), Some(index)) = (
        row.as_mut(),
        attribute(e, b"Index").and_then(|i| i.parse::<usize>().ok()),
    ) else {
        return;
    };
    while r.len() + 1 < index {
        r.push(String::new());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = r#"<?xml version="1.0"?>
<ss:Workbook xmlns:ss="urn:schemas-microsoft-com:office:spreadsheet">
<ss:Worksheet ss:Name="First">
<ss:Table>
<ss:Row><ss:Cell><ss:Data ss:Type="String">A &amp; B</ss:Data></ss:Cell><ss:Cell ss:Index="3"><ss:Data ss:Type="Number">1.5</ss:Data></ss:Cell></ss:Row>
<ss:Row/>
<ss:Row><ss:Cell/><ss:Cell><ss:Data ss:Type="String">x</ss:Data></ss:Cell></ss:Row>
</ss:Table>
</ss:Worksheet>
<ss:Worksheet ss:Name="Second"><ss:Table></ss:Table></ss:Worksheet>
</ss:Workbook>"#;

    #[test]
    fn reads_sheets_rows_and_cells() {
        let book = XmlSheets::open(DOC).unwrap();
        assert_eq!(book.names().collect::<Vec<_>>(), vec!["First", "Second"]);
        let first = book.sheet("First").unwrap();
        assert_eq!(first.rows[0], vec!["A & B", "", "1.5"]);
        assert!(first.rows[1].is_empty());
        assert_eq!(first.rows[2], vec!["", "x"]);
    }

    #[test]
    fn a_bare_ampersand_does_not_stop_the_read() {
        let doc = DOC.replace(
            "<ss:Table>",
            r#"<ss:Table ss:HRef="https://example.com/?a=1&b=2">"#,
        );
        let book = XmlSheets::open(&doc).unwrap();
        assert_eq!(book.sheet("First").unwrap().rows[0][0], "A & B");
        assert_eq!(
            escape_bare_ampersands("a &amp; b &#38; c & d"),
            "a &amp; b &#38; c &amp; d"
        );
    }

    #[test]
    fn other_xml_is_not_this_container() {
        assert!(matches!(
            XmlSheets::open("<?xml version=\"1.0\"?><a/>"),
            Err(DecodeError::NotThisContainer(_))
        ));
    }
}
