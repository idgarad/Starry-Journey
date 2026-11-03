//! Starry Journey library crate.
//!
//! Holds the platform-independent parts of the rebuild described in
//! `docs/Rebuild Prompt.md`. The `model` module is the "pure city model"
//! component: no windowing or graphics dependencies, so it can be tested in
//! isolation. `raster` turns a city into a logical-resolution RGBA buffer,
//! also without graphics dependencies. The binary (`main.rs`) owns windowing
//! and GPU presentation.

pub mod model;
pub mod raster;
