#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
// We deliberately use `map_or_else` instead of `unwrap_or_else` everywhere so
// an audit can grep the codebase for any `unwrap*` spelling.
#![allow(
    clippy::unnecessary_option_map_or_else,
    clippy::unnecessary_result_map_or_else,
    clippy::map_or_identity
)]

pub mod app;
pub mod cloud;
pub mod error;
pub mod parser;
pub mod state;
pub mod ui;
