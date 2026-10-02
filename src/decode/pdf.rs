use chrono::NaiveDate;
use pdf_extract::{Document, MediaBox, OutputDev, OutputError, Transform};
use std::cell::OnceCell;

use crate::decode::DecodeError;

/// One laid-out glyph and the box it occupies on the page.
#[derive(Debug, Clone)]
pub struct CharItem {
    pub text: String,
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
    /// Whether the glyph sits on the page the normal way up.
    ///
    /// Rotated glyphs have an off-diagonal text matrix (`m11`/`m22` near zero,
    /// `m12`/`m21` near ±1). CAMS stamps a generation watermark as vertical
    /// text down the margin, and a stray watermark glyph landing within the
    /// y-tolerance of a content line silently fuses onto it. Recording the
    /// orientation rather than discarding the glyph lets each parser decide:
    /// CAMS filters these out, SBI keeps everything.
    pub upright: bool,
}

pub struct SpatialOutputDev {
    pub pages: Vec<Vec<CharItem>>,
    current_page: Vec<CharItem>,
    flip_ctm: Transform,
    /// For each glyph in `pages`, whether the content stream set the text
    /// position before drawing it (`Tm`, `Td`, `TD`, `T*`) rather than the
    /// glyph following on from the one before.
    ///
    /// Kept beside the glyphs rather than on `CharItem`, which is public and
    /// built field by field: a new field there would break every caller that
    /// constructs one.
    pen_moves: Vec<Vec<bool>>,
    current_moves: Vec<bool>,
    pen_moved: bool,
}

impl Default for SpatialOutputDev {
    fn default() -> Self {
        Self::new()
    }
}

impl SpatialOutputDev {
    pub fn new() -> Self {
        Self {
            pages: Vec::new(),
            current_page: Vec::new(),
            flip_ctm: Transform::default(),
            pen_moves: Vec::new(),
            current_moves: Vec::new(),
            pen_moved: true,
        }
    }
}

impl OutputDev for SpatialOutputDev {
    fn begin_page(
        &mut self,
        _page_num: u32,
        media_box: &MediaBox,
        _: Option<(f64, f64, f64, f64)>,
    ) -> Result<(), OutputError> {
        self.current_page.clear();
        self.current_moves.clear();
        self.pen_moved = true;
        self.flip_ctm = Transform::row_major(1., 0., 0., -1., 0., media_box.ury - media_box.lly);
        Ok(())
    }

    fn end_page(&mut self) -> Result<(), OutputError> {
        self.pages.push(self.current_page.clone());
        self.pen_moves.push(self.current_moves.clone());
        Ok(())
    }

    fn output_character(
        &mut self,
        trm: &Transform,
        width: f64,
        _spacing: f64,
        font_size: f64,
        char: &str,
    ) -> Result<(), OutputError> {
        let position = trm.post_transform(&self.flip_ctm);
        let x = position.m31;
        let y = position.m32;

        let scaled_w = trm.m11 * width * font_size;
        let scaled_h = trm.m22 * font_size;

        self.current_page.push(CharItem {
            text: char.to_string(),
            x0: x,
            y0: y,
            x1: x + scaled_w.abs(),
            y1: y + scaled_h.abs(),
            upright: trm.m12.abs() <= 0.5 && trm.m21.abs() <= 0.5,
        });
        self.current_moves.push(std::mem::take(&mut self.pen_moved));
        Ok(())
    }

    fn begin_word(&mut self) -> Result<(), OutputError> {
        Ok(())
    }
    fn end_word(&mut self) -> Result<(), OutputError> {
        Ok(())
    }
    /// Called on every operator that sets the text position.
    fn end_line(&mut self) -> Result<(), OutputError> {
        self.pen_moved = true;
        Ok(())
    }
}

/// Every page laid out, with where the content stream moved the pen.
struct Layout {
    glyphs: Vec<Vec<CharItem>>,
    pen_moves: Vec<Vec<bool>>,
}

/// A PDF opened and decrypted once, with its expensive derivations cached.
///
/// Loading the document is the costly half of reading a PDF, and detection
/// would otherwise pay it once per candidate parser. `page1_text` is the cheap
/// view probes use to recognise an institution; `pages` is the full spatial
/// layout a parser needs.
pub struct PdfDoc {
    doc: Document,
    layout: OnceCell<Result<Layout, DecodeError>>,
    page1_text: OnceCell<String>,
}

impl PdfDoc {
    pub fn open(bytes: &[u8], password: Option<&str>) -> Result<Self, DecodeError> {
        let mut doc = Document::load_mem(bytes)
            .map_err(|e| DecodeError::NotThisContainer(format!("Failed to load PDF: {:?}", e)))?;
        if let Some(pw) = password {
            doc.decrypt(pw)
                .map_err(|_| DecodeError::IncorrectPassword)?;
        } else if doc.is_encrypted() {
            return Err(DecodeError::PasswordRequired);
        }
        Ok(Self {
            doc,
            layout: OnceCell::new(),
            page1_text: OnceCell::new(),
        })
    }

    /// Every glyph on every page, with its position and orientation.
    pub fn pages(&self) -> Result<&[Vec<CharItem>], DecodeError> {
        self.layout().map(|layout| layout.glyphs.as_slice())
    }

    /// For each glyph `pages` returns, whether the content stream set the text
    /// position before drawing it rather than letting it follow on from the
    /// glyph before.
    ///
    /// A generator positions each run of text it lays out, so this is where
    /// the producer said one piece of text ends and another begins. It holds
    /// when the glyph positions do not: a font whose widths cannot be read
    /// is advanced by a guessed width, which moves every glyph after the
    /// first in a run but not the point the run was placed at.
    // Only the forex card reader needs this yet; the other PDF parsers cut
    // their text by position alone.
    #[cfg_attr(not(feature = "rt-sbi-forex-card"), allow(dead_code))]
    pub(crate) fn pen_moves(&self) -> Result<&[Vec<bool>], DecodeError> {
        self.layout().map(|layout| layout.pen_moves.as_slice())
    }

    fn layout(&self) -> Result<&Layout, DecodeError> {
        self.layout
            .get_or_init(|| {
                let mut out = SpatialOutputDev::new();
                // The document opened, so a failure here is damage rather than
                // a wrong guess about the format.
                pdf_extract::output_doc(&self.doc, &mut out)
                    .map_err(|e| DecodeError::Damaged(format!("Extraction failed: {:?}", e)))?;
                Ok(Layout {
                    glyphs: out.pages,
                    pen_moves: out.pen_moves,
                })
            })
            .as_ref()
            .map_err(Clone::clone)
    }

    /// The date the PDF's own metadata says it was produced.
    ///
    /// Document metadata, not the filesystem's -- a copied or re-downloaded
    /// file keeps it. That makes it usable where the printed date is real but
    /// ambiguous: a sheet that prints "7/3/2020" has told you the day and the
    /// month without telling you which is which, and this says which.
    ///
    /// Only ever a tie-break. What a document prints about itself outranks
    /// what its producer stamped into the file, and a parser that reached for
    /// this first would be reading the generator rather than the statement.
    pub fn creation_date(&self) -> Option<NaiveDate> {
        let info = self.doc.trailer.get(b"Info").ok()?;
        let dict = match info.as_reference() {
            Ok(id) => self.doc.get_dictionary(id).ok()?,
            Err(_) => info.as_dict().ok()?,
        };
        let raw = dict.get(b"CreationDate").ok()?.as_str().ok()?;
        // "D:YYYYMMDDHHmmSSOHH'mm'" -- only the date half is wanted, and the
        // prefix and everything after the day are optional in the wild.
        let text = std::str::from_utf8(raw).ok()?;
        let digits: String = text
            .trim_start_matches("D:")
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        if digits.len() < 8 {
            return None;
        }
        NaiveDate::parse_from_str(&digits[..8], "%Y%m%d").ok()
    }

    /// Plain text of page 1, for probing.
    ///
    /// An institution identifies itself on the first page, so this is all a
    /// probe needs and it avoids laying out a forty-page statement to answer
    /// "is this yours?". Returns an empty string if the page cannot be laid
    /// out -- a probe should decline, not fail the whole parse.
    pub fn page1_text(&self) -> &str {
        self.page1_text.get_or_init(|| {
            let mut text = String::new();
            {
                let mut out = pdf_extract::PlainTextOutput::new(&mut text);
                if pdf_extract::output_doc_page(&self.doc, &mut out, 1).is_err() {
                    return String::new();
                }
            }
            text
        })
    }
}
