# Core tests

Unit tests live in `unit/`, organized by the source module they exercise. Source
modules include them with `#[cfg(test)]` and an explicit `#[path]`; this keeps
the tests in one tree while preserving access to module-private implementation
details.

Rust integration tests live directly in this directory as `.rs` files and
exercise the public crate API. Cargo discovers those files as separate test
targets.
