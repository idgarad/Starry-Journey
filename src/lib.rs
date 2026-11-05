//! Starry Journey library crate.
//!
//! Holds the platform-independent parts of the rebuild described in
//! `docs/Rebuild Prompt.md`. The `model` module is the "pure city model"
//! component: no windowing or graphics dependencies, so it can be tested in
//! isolation. `raster` turns a city into a logical-resolution RGBA buffer,
//! also without graphics dependencies. The binary (`main.rs`) owns windowing
//! and GPU presentation.

pub mod ambience;
pub mod cityglow;
pub mod clock;
pub mod cloudgen;
pub mod clouds;
pub mod config;
pub mod endless;
pub mod lightning;
pub mod message;
pub mod model;
pub mod moon;
pub mod quips;
pub mod rain;
pub mod raster;
pub mod reveal;
pub mod searchlights;
pub mod skyclouds;
pub mod snapshot;
pub mod station;
pub mod street;
pub mod traffic;
pub mod weather;
