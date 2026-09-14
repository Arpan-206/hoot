//! Protocol helpers for Sprig OS.
//!
//! Everything here is plain data processing with no hardware and no heap,
//! so it runs on the RP2040 and in host unit tests (`cargo host-test`).

#![cfg_attr(not(test), no_std)]
#![deny(unsafe_code)]

pub mod album;
pub mod crc32;
pub mod dhcp;
pub mod dns;
pub mod form;
pub mod http;
pub mod messages;
pub mod record;
pub mod time;
pub mod url;
