//! `joblode-core` — search, retrieval, and ranking over the open-jobs dataset.
//!
//! Phase 0 is a skeleton: it establishes the crate and the test harness only.
//! Phase 1 introduces the DuckDB-backed [`search`] API and its query types; see
//! `docs/DESIGN.md` for the plan.

/// The crate version, reported as a trivial smoke surface until Phase 1 lands the
/// real query API. Replace once `search`/`get_job` exist.
#[must_use]
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_its_version() {
        assert_eq!(version(), env!("CARGO_PKG_VERSION"));
    }
}
