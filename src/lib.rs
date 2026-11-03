//! Starry Journey library crate.
//!
//! Holds the platform-independent parts of the rebuild described in
//! `docs/Rebuild Prompt.md`. The `model` module is the "pure city model"
//! component: no windowing or graphics dependencies, so it can be tested in
//! isolation. The binary (`main.rs`) owns windowing and rendering.

pub mod model;
