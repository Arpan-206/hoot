//! Shared user-interface pieces: theme, text formatting, splash and shell.

#[cfg(feature = "wifi")]
pub mod qr;
#[cfg(feature = "wifi")]
pub mod setup;
pub mod shell;
pub mod splash;
pub mod text;
pub mod theme;
