//! The interactive browser UI behind `data-diff --ui`: a small
//! dependency-free HTTP server holding one diff session and serving the
//! Preact frontend (`ui/dist`) to the browser.

pub mod commands;
pub mod dev;
pub mod dto;
pub mod embedded;
pub mod http;
pub mod serve;
pub mod session;
