//! On Air Record: a cross platform audio broadcast and DVR service.
//!
//! The crate is split into a library and a thin binary so the layers can be exercised by tests and, in
//! time, reused by another front end. See `docs/ARCHITECTURE.md` for how the layers fit together.

pub mod app;
pub mod audio;
pub mod config;
pub mod controllers;
pub mod db;
pub mod dto;
pub mod error;
pub mod models;
pub mod repositories;
pub mod routes;
pub mod services;
pub mod util;
pub mod ws;
