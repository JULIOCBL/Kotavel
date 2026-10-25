//! Rust migration of Kotavel.
//!
//! Modules are moved into this crate in small, compilable increments. Public
//! behavior is kept stable while platform and framework adapters are replaced.

pub mod config;
pub mod console;
pub mod container;
pub mod contracts;
pub mod desktop;
pub mod diagnostics;
pub mod env;
pub mod filesystem;
pub mod foundation;
pub mod http;
pub mod metrics;
pub mod network;
pub mod platform_services;
pub mod runtime_paths;
pub mod session;
pub mod system;
pub mod system_paths;
pub mod text;
