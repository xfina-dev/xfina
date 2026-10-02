//! Published price history: an exchange's prices, a fund's NAVs, an index's
//! levels.
//!
//! One module per publisher, each reading that publisher's own download into
//! a [`PriceSeries`](crate::models::PriceSeries). Each reads one file and only
//! that file: a dataset that arrives in pieces -- a year of NSE index history
//! per download, five years of AMFI NAVs -- comes back as one series per
//! piece, and putting the pieces together is left to whoever holds them all.

#[cfg(feature = "md-amfi-nav")]
pub mod amfi;
#[cfg(feature = "md-ishares")]
pub mod ishares;
#[cfg(feature = "md-mcx-spot")]
pub mod mcx;
#[cfg(feature = "md-msci")]
pub mod msci;
#[cfg(feature = "md-nasdaq")]
pub mod nasdaq;
#[cfg(feature = "md-nse-indices")]
pub mod nse_indices;
#[cfg(feature = "md-nse-security")]
pub mod nse_security;
#[cfg(feature = "md-spdr-gold")]
pub mod spdr;
#[cfg(feature = "md-tiingo")]
pub mod tiingo;
#[cfg(feature = "md-wsj")]
pub mod wsj;
#[cfg(feature = "md-yahoo")]
pub mod yahoo;

#[cfg(any(
    feature = "md-amfi-nav",
    feature = "md-ishares",
    feature = "md-mcx-spot",
    feature = "md-msci",
    feature = "md-nasdaq",
    feature = "md-nse-indices",
    feature = "md-nse-security",
    feature = "md-spdr-gold",
    feature = "md-tiingo",
    feature = "md-wsj",
    feature = "md-yahoo",
))]
mod common;
