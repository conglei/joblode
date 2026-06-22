use std::path::PathBuf;

use joblode_core::{Criteria, JobStore};

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/fixture.parquet")
}

fn search(criteria: Criteria) -> (Vec<String>, usize) {
    let store = JobStore::open(fixture()).expect("fixture should open");
    let (jobs, total) = store.search(&criteria).expect("search should succeed");
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
        .expect("fixture job should exist");

    assert_eq!(job.company, "Acme");
    assert_eq!(job.title, "Backend Engineer");
    assert_eq!(job.jd_markdown, "# Backend Engineer");
}
