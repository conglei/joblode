use std::path::PathBuf;

use joblode_core::{Criteria, JobStore};

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/fixture.parquet")
}

/// Larger than the fixture, so the default helper returns every match.
const ALL: usize = 1000;

fn search(criteria: Criteria) -> (Vec<String>, usize) {
    search_limited(criteria, ALL)
}

fn search_limited(criteria: Criteria, limit: usize) -> (Vec<String>, usize) {
    let store = JobStore::open(fixture()).expect("fixture should open");
    let (jobs, total) = store
        .search(&criteria, limit)
        .expect("search should succeed");
    (jobs.into_iter().map(|job| job.id).collect(), total)
}

#[test]
fn filters_city_across_city_region_and_location() {
    let (ids, total) = search(Criteria {
        cities: vec!["san francisco".into()],
        ..Criteria::default()
    });

    assert_eq!(ids, ["city-direct", "city-location", "city-region"]);
    assert_eq!(total, 3);
}

#[test]
fn filters_function() {
    let (ids, total) = search(Criteria {
        functions: vec!["product".into()],
        ..Criteria::default()
    });

    assert_eq!(ids, ["city-region"]);
    assert_eq!(total, 1);
}

#[test]
fn filters_level() {
    let (ids, total) = search(Criteria {
        levels: vec!["Junior".into()],
        ..Criteria::default()
    });

    assert_eq!(ids, ["city-location"]);
    assert_eq!(total, 1);
}

#[test]
fn filters_title_by_case_insensitive_substring() {
    let (ids, total) = search(Criteria {
        titles: vec!["BACKEND".into(), "product manager".into()],
        ..Criteria::default()
    });

    assert_eq!(ids, ["city-direct", "city-region"]);
    assert_eq!(total, 2);
}

#[test]
fn filters_company_across_canonical_and_raw_names() {
    let (ids, total) = search(Criteria {
        companies: vec!["remoteco".into(), "staffing feed".into()],
        ..Criteria::default()
    });

    assert_eq!(ids, ["dedup-first", "us-scope"]);
    assert_eq!(total, 2);
}

#[test]
fn combines_title_and_company_filters() {
    let (ids, total) = search(Criteria {
        titles: vec!["engineer".into()],
        companies: vec!["datahigh".into()],
        ..Criteria::default()
    });

    assert_eq!(ids, ["comp-high"]);
    assert_eq!(total, 1);
}

#[test]
fn accepts_multiple_values_within_search_criteria() {
    let (ids, total) = search(Criteria {
        functions: vec!["engineering".into(), "product".into()],
        levels: vec!["Mid".into(), "Staff".into()],
        cities: vec!["San Francisco".into(), "Remote".into()],
        ..Criteria::default()
    });

    assert_eq!(ids, ["city-region", "us-scope"]);
    assert_eq!(total, 2);
}

#[test]
fn treats_us_remote_scopes_as_us_jobs() {
    let (ids, total) = search(Criteria {
        country: Some("US".into()),
        functions: vec!["engineering".into()],
        levels: vec!["Staff".into()],
        ..Criteria::default()
    });

    assert_eq!(ids, ["us-scope"]);
    assert_eq!(total, 1);
}

#[test]
fn keeps_unknown_compensation_above_a_comp_floor() {
    let (ids, total) = search(Criteria {
        functions: vec!["data".into()],
        levels: vec!["Principal".into()],
        min_comp: Some(150.0),
        ..Criteria::default()
    });

    assert_eq!(ids, ["comp-high", "comp-unknown"]);
    assert_eq!(total, 2);
}

#[test]
fn deduplicates_company_and_title_case_insensitively() {
    let (ids, total) = search(Criteria {
        functions: vec!["engineering".into()],
        levels: vec!["Lead".into()],
        ..Criteria::default()
    });

    assert_eq!(ids, ["dedup-first"]);
    assert_eq!(total, 1);
}

#[test]
fn caps_returned_rows_but_reports_the_full_total() {
    // Three SF roles match; a limit of 1 returns one row but the full total.
    let (ids, total) = search_limited(
        Criteria {
            cities: vec!["san francisco".into()],
            ..Criteria::default()
        },
        1,
    );

    assert_eq!(ids, ["city-direct"]);
    assert_eq!(total, 3);
}

#[test]
fn returns_empty_results() {
    let (ids, total) = search(Criteria {
        cities: vec!["Tokyo".into()],
        ..Criteria::default()
    });

    assert!(ids.is_empty());
    assert_eq!(total, 0);
}

#[test]
fn gets_a_job_with_its_full_description() {
    let store = JobStore::open(fixture()).expect("fixture should open");

    let job = store
        .get_job("city-direct")
        .expect("query should succeed")
        .expect("fixture job should exist");

    assert_eq!(job.company, "Acme");
    assert_eq!(job.title, "Backend Engineer");
    assert_eq!(job.jd_markdown, "# Backend Engineer");
}

#[test]
fn returns_none_for_a_missing_job() {
    let store = JobStore::open(fixture()).expect("fixture should open");

    let result = store
        .get_job("not-a-real-job-id")
        .expect("query should succeed");

    assert!(result.is_none());
}
