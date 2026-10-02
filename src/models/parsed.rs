//! What a parse produced, when not every format produces an account.

use serde_json::Value;

use crate::error::XfinaError;
use crate::models::account::Account;

use crate::models::rates::RateSheet;
use crate::models::schema::Schema;
use crate::models::series::PriceSeries;
use crate::models::{CreditCardAccount, DepositAccount, EquityAccount, MutualFundsAccount};

/// The result of reading a file, whatever kind of document it turned out to be.
///
/// Every format used to produce an [`Account`], and most still do. A rate
/// sheet is not an account, a holding or a transaction, so rather than give it
/// a hollow account to live in -- an account with no holder, no balance and no
/// number -- it gets its own variant and the registry stays one table. A
/// published price history is the same kind of thing, and gets the same.
#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub enum Parsed {
    Account(Account),
    Rates(RateSheet),
    Series(PriceSeries),
}

impl Parsed {
    /// Renders into the requested [`Schema`].
    ///
    /// # Errors
    ///
    /// [`XfinaError::SchemaUnsupported`] when the schema has no way to express
    /// the document. ReBIT describes accounts held by a person; it has no term
    /// for a price an institution published, so asking for one is a question
    /// with no answer rather than a document with empty fields.
    pub fn to_json(&self, schema: Schema) -> Result<Value, XfinaError> {
        match self {
            Parsed::Account(account) => Ok(account.to_json(schema)),
            Parsed::Rates(sheet) => sheet.to_json(schema),
            Parsed::Series(series) => series.to_json(schema),
        }
    }

    /// The parsed account, for a caller that only handles accounts.
    pub fn account(&self) -> Option<&Account> {
        match self {
            Parsed::Account(account) => Some(account),
            Parsed::Rates(_) | Parsed::Series(_) => None,
        }
    }

    /// The parsed rate sheet, for a caller that only handles rate sheets.
    pub fn rates(&self) -> Option<&RateSheet> {
        match self {
            Parsed::Rates(sheet) => Some(sheet),
            Parsed::Account(_) | Parsed::Series(_) => None,
        }
    }

    /// The parsed price series, for a caller that only handles market data.
    pub fn series(&self) -> Option<&PriceSeries> {
        match self {
            Parsed::Series(series) => Some(series),
            Parsed::Account(_) | Parsed::Rates(_) => None,
        }
    }
}

impl From<Account> for Parsed {
    fn from(a: Account) -> Self {
        Parsed::Account(a)
    }
}

impl From<RateSheet> for Parsed {
    fn from(r: RateSheet) -> Self {
        Parsed::Rates(r)
    }
}

impl From<PriceSeries> for Parsed {
    fn from(s: PriceSeries) -> Self {
        Parsed::Series(s)
    }
}

/// The registry's generated dispatch hands a parser's own model straight to
/// `Parsed::from`, so each account model needs the two-step conversion spelled
/// out. Without these the macro would have to know which formats are accounts,
/// which is exactly the per-row knowledge it exists to avoid.
macro_rules! impl_parsed_from_account {
    ($($ty:ty),+ $(,)?) => {$(
        impl From<$ty> for Parsed {
            fn from(a: $ty) -> Self {
                Parsed::Account(Account::from(a))
            }
        }
    )+};
}

impl_parsed_from_account!(
    DepositAccount,
    CreditCardAccount,
    MutualFundsAccount,
    EquityAccount,
);
