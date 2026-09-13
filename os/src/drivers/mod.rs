//! Device drivers. Each driver is generic over `embedded-hal` traits where
//! that is practical, so it can be reused or unit-tested with fakes later.

pub mod dimmer;
pub mod input;
pub mod module;
pub mod power;
pub mod st7735;
