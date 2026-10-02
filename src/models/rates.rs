//! A published rate sheet: prices an institution quotes, not an account.
//!
//! Nothing here is ReBIT-shaped, because ReBIT has nothing to say about a
//! reference document. It describes no holder, no balance and no transaction,
//! so it carries its own small structure rather than being bent into one of
//! the four account models.

use std::collections::BTreeMap;

use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::error::XfinaError;
use crate::models::schema::Schema;

/// One day's published rate card.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RateSheet {
    /// The date the sheet prints on itself.
    ///
    /// Never the clock and never the filename: a sheet fetched late, or
    /// archived under a name someone chose, still has exactly one date that is
    /// true of it, and it is the one the institution wrote.
    pub date: NaiveDate,
    /// The moment the sheet was published, when it prints a time as well.
    ///
    /// Quoted in IST, as everything an Indian institution publishes is, and
    /// converted to UTC here so a caller never has to guess the offset.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub published_at: Option<DateTime<Utc>>,
    /// The rate columns this sheet printed, in the order it printed them.
    ///
    /// Sheets rename and retire columns between layout eras, so the set is a
    /// property of the document rather than of the institution. Naming them
    /// here means a caller can see what a given day actually quoted instead of
    /// inferring it from which keys happen to be present.
    pub columns: Vec<String>,
    /// One entry per currency the sheet quotes, in the order it lists them.
    pub currencies: Vec<CurrencyRates>,
    /// Set when the figures could not be placed under their headings and were
    /// matched to them by order instead.
    ///
    /// A few sheets are published with the table collapsed into a flow, where
    /// no two rows begin at the same place and nothing lines up under a
    /// heading. The figures are all there and in order, and are only read this
    /// way when every row accounts for every heading exactly once -- but a
    /// caller holding these rates to a higher standard can tell them apart
    /// from the ones the page itself vouched for.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub figures_matched_by_order: bool,
}

/// Every rate one sheet quotes for one currency.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CurrencyRates {
    /// ISO 4217 code, as the sheet prints it in `XXX/INR`.
    pub currency: String,
    /// The currency's name in full, as printed.
    pub name: String,
    /// How many units of the foreign currency the quote is for.
    ///
    /// Some currencies are quoted per 100 units, and a caller that assumes 1
    /// is out by two orders of magnitude with nothing in the data to warn it.
    /// Read from the sheet's own note rather than hardcoded, so a change to
    /// which currencies are grouped this way follows the document.
    pub unit: u32,
    /// Rate by column key. A column the sheet left unquoted is absent rather
    /// than zero -- a rate of zero is not a price, and publishing it as one is
    /// how a "0.0" ends up in a downstream time series.
    #[serde(with = "crate::models::decimal_map")]
    pub rates: BTreeMap<String, Decimal>,
}

impl CurrencyRates {
    /// The rate under `column`, if the sheet quoted one.
    pub fn get(&self, column: &str) -> Option<Decimal> {
        self.rates.get(column).copied()
    }
}

impl RateSheet {
    /// The entry for `currency` (an ISO code such as `"USD"`), if quoted.
    pub fn currency(&self, currency: &str) -> Option<&CurrencyRates> {
        self.currencies.iter().find(|c| c.currency == currency)
    }

    /// Renders into the requested [`Schema`].
    ///
    /// # Errors
    ///
    /// [`XfinaError::SchemaUnsupported`] for a schema with no way to express a
    /// rate sheet. There is no `date_only_paths` equivalent here because the
    /// only schema that needs one is the only schema this refuses.
    pub fn to_json(&self, schema: Schema) -> Result<serde_json::Value, XfinaError> {
        match schema {
            Schema::Xfina => Ok(crate::models::serializer::render(self, schema, &[])),
            Schema::Rebit => Err(XfinaError::SchemaUnsupported {
                schema: schema.as_str(),
                document: "reference rate sheet",
            }),
        }
    }
}
