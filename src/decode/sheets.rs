use calamine::{open_workbook_auto_from_rs, Data, Range, Reader};
use std::io::Cursor;

use crate::decode::DecodeError;

/// A workbook read once into owned ranges.
///
/// `calamine::Reader::worksheets` hands back fully owned `Range<Data>` values,
/// so the reader does not have to be kept alive and every parser can share one
/// read of the file. `open_workbook_auto_from_rs` dispatches on the content,
/// which is what lets an ICICI card export named `.xls` -- really an xlsx --
/// be read without the caller knowing.
pub struct Sheets {
    sheets: Vec<(String, Range<Data>)>,
}

impl Sheets {
    pub fn open(bytes: &[u8]) -> Result<Self, DecodeError> {
        let mut workbook = open_workbook_auto_from_rs(Cursor::new(bytes)).map_err(|e| {
            DecodeError::NotThisContainer(format!("Failed to open workbook: {}", e))
        })?;
        Ok(Self {
            sheets: workbook.worksheets(),
        })
    }

    /// The first sheet, which is where every statement we read puts its data.
    pub fn first(&self) -> Result<&Range<Data>, DecodeError> {
        self.sheets
            .first()
            .map(|(_, range)| range)
            .ok_or_else(|| DecodeError::NotThisContainer("No sheets found in workbook".to_string()))
    }

    /// The sheet with this exact name, for workbooks that do not put their
    /// data first.
    pub fn get(&self, name: &str) -> Option<&Range<Data>> {
        self.sheets
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, range)| range)
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.sheets.iter().map(|(name, _)| name.as_str())
    }

    pub fn is_empty(&self) -> bool {
        self.sheets.is_empty()
    }

    /// Uppercased text of the first `rows` rows of the first sheet, for probes.
    ///
    /// A statement identifies itself in its header block, so a probe never has
    /// to walk the transaction table.
    pub fn head_text(&self, rows: usize) -> String {
        let Ok(range) = self.first() else {
            return String::new();
        };
        let mut out = String::new();
        for row in range.rows().take(rows) {
            for cell in row {
                // Every cell type, normalised the way the parsers do: some
                // exports carry their labels in non-string cells and pad them
                // with NULs, so reading only Data::String would miss the
                // header entirely.
                let text = cell.to_string();
                let text = text.replace('\u{0}', "");
                let text = text.trim();
                if !text.is_empty() {
                    out.push_str(text);
                    out.push('\n');
                }
            }
        }
        out.to_uppercase()
    }
}
