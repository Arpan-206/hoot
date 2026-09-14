//! Graphics primitives for Hoot.
//!
//! This crate has no hardware dependencies. It compiles for the RP2040 and
//! for the host, so all drawing code can be unit-tested with `cargo host-test`.

#![cfg_attr(not(test), no_std)]
#![deny(unsafe_code)]

mod color;
mod font;
mod framebuffer;

pub use color::Rgb565;
pub use font::{CELL_HEIGHT, CELL_WIDTH, GLYPH_HEIGHT, GLYPH_WIDTH};
pub use framebuffer::{BYTES, Framebuffer, HEIGHT, WIDTH};
