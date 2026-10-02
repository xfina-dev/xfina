//! Turning a page of the forex card rate sheet into the cells its table is
//! made of.
//!
//! A rate card is a grid with no text flow: what a number means is decided by
//! which column it sits under, not by where it falls in the reading order. So
//! this keeps geometry rather than producing lines of text, and the parser
//! matches values to headings by their positions on the page.

use crate::decode::CharItem;

/// One run of glyphs laid out as a unit: a heading, a currency name, a number.
#[derive(Debug, Clone)]
pub struct Cell {
    pub text: String,
    pub x0: f64,
    pub x1: f64,
    /// Baseline the cell was set on, used to keep a run on one line.
    pub y0: f64,
    /// Vertical middle of the cell.
    ///
    /// The middle rather than the baseline because a row's label and its
    /// figures are not always set on the same baseline -- one layout era
    /// offsets them by a few points -- while their centres stay together.
    pub yc: f64,
}

/// Splits a page into cells.
///
/// Glyphs are gathered onto shared baselines and then cut wherever the gap
/// between them exceeds a fraction of the type size. Cutting on a measured gap
/// rather than on the space character keeps "TT BUY" and "UNITED STATES
/// DOLLAR" whole -- a heading split at its space would have to be glued back
/// together by guessing which fragments belong to each other.
///
/// A cell also ends wherever the content stream moved the pen, as
/// `pen_moves` records for each glyph. That is the producer placing a new
/// piece of text, and it holds where the measured gap does not: on the sheets
/// whose glyph widths cannot be read, every run is drawn about twice its real
/// width, and a figure placed just after another lands inside it.
pub fn cells(page: &[CharItem], pen_moves: &[bool]) -> Vec<Cell> {
    // Grouped in the order the glyphs were written, not in the order they sit
    // across the page. Some sheets are published with the wrong advance
    // widths, which draws two headings over each other; sorting by position
    // first interleaves their letters beyond recovery, while the writing order
    // still has each heading whole. Position decides what a cell *means*, but
    // only after the cell has been assembled.
    let mut out: Vec<Cell> = Vec::new();
    let mut current: Option<Cell> = None;

    for (glyph, &pen_moved) in page.iter().zip(pen_moves) {
        // Rotated glyphs belong to watermarks and stamps, never to the table.
        // One landing within a row's tolerance would fuse onto a number.
        if !glyph.upright {
            out.extend(current.take());
            continue;
        }
        let height = (glyph.y1 - glyph.y0).abs();
        let continues = !pen_moved
            && current.as_ref().is_some_and(|cell| {
                (glyph.y0 - cell.y0).abs() <= BASELINE_TOLERANCE
                    && glyph.x0 >= cell.x1 - height
                    && glyph.x0 - cell.x1 <= GAP_RATIO * height
            });
        match current.as_mut() {
            Some(cell) if continues => {
                cell.text.push_str(&glyph.text);
                cell.x1 = cell.x1.max(glyph.x1);
            }
            _ => {
                out.extend(current.take());
                current = Some(Cell {
                    text: glyph.text.clone(),
                    x0: glyph.x0,
                    x1: glyph.x1,
                    y0: glyph.y0,
                    yc: (glyph.y0 + glyph.y1) / 2.0,
                });
            }
        }
    }
    out.extend(current);
    out
}

/// Glyphs this far apart vertically are on the same baseline.
///
/// Read the same across the archive anywhere from 1.0 to 3.5.
const BASELINE_TOLERANCE: f64 = 2.0;

/// A gap wider than this many times the type size ends a cell.
///
/// Spaces inside a heading are written as glyphs of their own, so a cell holds
/// together however small this is; what it has to stay under is the gutter
/// between two columns. Swept across the archive, anything from 0.10 to 0.45
/// reads every sheet identically and 0.50 merges adjacent columns on 791 of
/// them. Set near the bottom of that range rather than the middle, because the
/// two failures are not equal: too small splits a heading, which is caught and
/// refused, while too large silently joins two columns into one.
const GAP_RATIO: f64 = 0.25;

#[cfg(test)]
mod tests {
    use super::*;

    fn glyphs(text: &str, x: f64) -> Vec<CharItem> {
        text.chars()
            .enumerate()
            .map(|(i, ch)| CharItem {
                text: ch.to_string(),
                x0: x + 5.0 * i as f64,
                x1: x + 5.0 * (i + 1) as f64,
                y0: 100.0,
                y1: 109.0,
                upright: true,
            })
            .collect()
    }

    #[test]
    fn a_moved_pen_ends_a_cell_that_its_position_would_continue() {
        // "21.55" and "23.70" set flush against each other: by position alone
        // they are one run, but the stream placed the second on its own.
        let mut page = glyphs("21.55", 0.0);
        page.extend(glyphs("23.70", 25.0));
        let mut moves = vec![false; page.len()];
        moves[0] = true;

        let joined: Vec<String> = cells(&page, &moves).into_iter().map(|c| c.text).collect();
        assert_eq!(joined, vec!["21.5523.70"]);

        moves[5] = true;
        let split: Vec<String> = cells(&page, &moves).into_iter().map(|c| c.text).collect();
        assert_eq!(split, vec!["21.55", "23.70"]);
    }
}
