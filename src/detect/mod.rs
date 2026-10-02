//! Working out what a statement is, before deciding how to read it.
//!
//! Three stages, in order of authority:
//!
//! 1. **Container** -- the magic bytes say whether this is an OLE2 workbook, a
//!    zip, a PDF or text. Formats that cannot appear in that container are
//!    dropped without being asked.
//! 2. **Filename** -- reorders what is left so the likely parser goes first.
//!    A hint never decides; a renamed file must still land correctly.
//! 3. **Content** -- each remaining format is asked whether the file is its
//!    own, against a decode that happens once no matter how many ask.
//!
//! Validation never enters into it. A statement whose totals do not reconcile
//! is still that institution's statement, and must come back labelled as such
//! with a failing report -- not silently relabelled as something else.

pub mod container;
pub mod hint;
pub mod probe;
pub mod registry;

pub use container::{sniff, Container};
pub use probe::{Claim, Probe, Strength};
pub use registry::{Area, Category, Format, FormatInfo};

#[cfg(feature = "_pdf")]
use crate::decode::DecodeError;
use crate::decode::Decoded;
use crate::error::XfinaError;
use crate::models::request::ParseRequest;
use crate::models::statement::{Candidate, Confidence, Detection, FileInfo};

/// The formats this build knows about, enabled or not.
///
/// Ordered by category, then institution alphabetically, so every surface --
/// the CLI listing, a picker built from this, a docs table -- reads the same
/// way without sorting it again. The registry table stays in whatever order
/// is convenient to maintain; presentation order is decided here.
pub fn formats() -> Vec<FormatInfo> {
    let mut all: Vec<FormatInfo> = Format::ALL.iter().copied().map(FormatInfo::from).collect();
    // `id` breaks the tie where one institution appears in two categories, so
    // the order is total and does not depend on the sort being stable.
    all.sort_by(|a, b| {
        a.category
            .cmp(&b.category)
            .then_with(|| a.institution.cmp(b.institution))
            .then_with(|| a.id.cmp(b.id))
    });
    all
}

/// Works out what a file is, without parsing it.
///
/// Cheaper than [`crate::parse`] and enough to label a file or route it. Note
/// that it stops after probing: where several formats are merely plausible it
/// reports [`Confidence::Weak`] and lists them, whereas `parse` goes on to try
/// them. A `Statement`'s detection can therefore be more resolved than what
/// this returns for the same bytes.
pub fn detect(input: &ParseRequest<'_>) -> Result<Detection, XfinaError> {
    let decoded = Decoded::new(input);
    resolve(&decoded, input)
}

pub(crate) fn resolve(
    decoded: &Decoded<'_>,
    input: &ParseRequest<'_>,
) -> Result<Detection, XfinaError> {
    let container = decoded.container();
    let filename_hint = hint::from_filename(input.filename);
    let file = FileInfo::of(input);

    // A caller-stated format skips detection entirely.
    if let Some(format) = input.format {
        if !format.is_enabled() {
            return Err(XfinaError::FormatNotEnabled(format.id()));
        }
        return Ok(Detection {
            format,
            confidence: Confidence::Stated,
            container,
            reason: "caller-stated",
            filename_hint,
            candidates: Vec::new(),
            file,
        });
    }

    let mut candidates: Vec<Candidate> = Vec::new();

    for format in Format::ALL.iter().copied() {
        if !format.is_enabled() || !format.containers().contains(&container) {
            continue;
        }
        let claim = registry::dispatch_probe(format, decoded);
        if claim.is_match() {
            candidates.push(Candidate {
                format,
                strength: claim.strength,
                reason: claim.reason,
            });
        }
    }

    // Failing to open a file outranks failing to recognise one. An encrypted
    // PDF has no readable content to probe, so every candidate declines it --
    // reporting that as "unrecognised" would send a caller looking for a
    // missing parser instead of asking for the password.
    if candidates.is_empty() {
        #[cfg(feature = "_pdf")]
        if container == Container::Pdf {
            if let Err(e @ (DecodeError::PasswordRequired | DecodeError::IncorrectPassword)) =
                decoded.pdf()
            {
                return Err(e.into());
            }
        }
        return Err(XfinaError::UnrecognizedFormat {
            container,
            filename: input.filename.map(str::to_string),
        });
    }

    // Strongest claim wins; the filename breaks a tie, then priority. Both
    // tie-breaks are total, so there is no ambiguous outcome to report.
    candidates.sort_by_key(|c| {
        (
            std::cmp::Reverse(c.strength),
            Some(c.format) != filename_hint,
            c.format.priority(),
        )
    });

    let winner = candidates.remove(0);
    Ok(Detection {
        format: winner.format,
        confidence: match winner.strength {
            Strength::Strong => Confidence::Strong,
            _ => Confidence::Weak,
        },
        container,
        reason: winner.reason,
        filename_hint,
        candidates,
        file,
    })
}

/// The format detection settled on, for callers that only want the answer.
pub fn detect_format(input: &ParseRequest<'_>) -> Result<Format, XfinaError> {
    detect(input).map(|d| d.format)
}
