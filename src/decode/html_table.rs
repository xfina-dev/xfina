//! The rows of an HTML `<table>`, as cell text.
//!
//! MCX's spot price export is not a spreadsheet at all: it is a bare HTML
//! table saved under an `.xls` name, which Excel happens to open. This reads
//! the table generated pages emit -- `<tr>` rows of `<th>` or `<td>` cells --
//! without pretending to be an HTML parser: tags inside a cell are dropped,
//! the common entities are decoded and whitespace is collapsed.

/// Every row of every table in the document, in order.
pub fn rows(html: &str) -> Vec<Vec<String>> {
    let lower = html.to_ascii_lowercase();
    let mut out = Vec::new();
    let mut pos = 0;
    while let Some(start) = find_tag(&lower, "tr", pos) {
        let end = lower[start..]
            .find("</tr")
            .map(|i| start + i)
            .unwrap_or(lower.len());
        out.push(cells(&html[start..end], &lower[start..end]));
        pos = end;
    }
    out
}

/// The cells of one row's markup.
fn cells(html: &str, lower: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut pos = 0;
    loop {
        let td = find_tag(lower, "td", pos);
        let th = find_tag(lower, "th", pos);
        let Some(open) = [td, th].into_iter().flatten().min() else {
            break;
        };
        // Content starts after the opening tag's '>'.
        let Some(gt) = lower[open..].find('>') else {
            break;
        };
        let content_start = open + gt + 1;
        let close = [
            lower[content_start..].find("</td"),
            lower[content_start..].find("</th"),
        ]
        .into_iter()
        .flatten()
        .min()
        .map(|i| content_start + i)
        .unwrap_or(lower.len());
        out.push(text_of(&html[content_start..close]));
        pos = close;
    }
    out
}

/// The position of the next `<name` opening tag at or after `from`, where the
/// name is followed by `>` or whitespace (so `<th` does not match `<thead`).
fn find_tag(lower: &str, name: &str, from: usize) -> Option<usize> {
    let needle = format!("<{}", name);
    let mut pos = from;
    while let Some(i) = lower[pos..].find(&needle) {
        let at = pos + i;
        let next = lower.as_bytes().get(at + needle.len()).copied();
        if matches!(
            next,
            Some(b'>') | Some(b' ') | Some(b'\t') | Some(b'\n') | Some(b'\r')
        ) {
            return Some(at);
        }
        pos = at + needle.len();
    }
    None
}

/// A cell's visible text: tags removed, entities decoded, whitespace collapsed.
fn text_of(markup: &str) -> String {
    let mut text = String::with_capacity(markup.len());
    let mut in_tag = false;
    for ch in markup.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                text.push(' ');
            }
            _ if !in_tag => text.push(ch),
            _ => {}
        }
    }
    let decoded = text
        .replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&");
    decoded.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_header_and_body_rows() {
        let html = r#"<table><thead><tr><th>A</th><th>B &amp; C</th></tr></thead>
            <tbody><TR><td style="x">1.00</td><td> <b>two</b>  words </td></TR></tbody></table>"#;
        assert_eq!(
            rows(html),
            vec![
                vec!["A".to_string(), "B & C".to_string()],
                vec!["1.00".to_string(), "two words".to_string()],
            ]
        );
    }

    #[test]
    fn thead_is_not_a_header_cell() {
        assert_eq!(
            rows("<thead><tr><th>x</th></tr></thead>"),
            vec![vec!["x".to_string()]]
        );
    }
}
