use std::fs;
use std::panic;

#[test]
fn a_panic_writes_a_report_carrying_the_log_tail() {
    let home = tempfile::tempdir().expect("temp dir");
    let home = home.path();
    let file = common::FileLog::for_binary("test", Some(home), common::LogLevel::Debug);
    let _guard = common::init_logging(common::LogLevel::Off, Some(file));

    tracing::info!(target: "common", "a line the report should carry");

    let result = panic::catch_unwind(|| panic!("deliberate test panic"));
    assert!(result.is_err(), "the panic must still propagate");

    let reports: Vec<_> = fs::read_dir(common::paths::log_dir(Some(home)).join("crashes"))
        .expect("crash directory exists")
        .flatten()
        .map(|entry| entry.path())
        .collect();
    assert_eq!(reports.len(), 1, "expected exactly one report: {reports:?}");

    let report = fs::read_to_string(&reports[0]).expect("report is readable");
    let paths = report.replace('\\', "/");
    assert!(report.contains("deliberate test panic"), "{report}");
    assert!(report.contains("kind:     panic"), "{report}");
    assert!(paths.contains("crates/common/tests/crash.rs"), "{report}");
    assert!(
        report.contains("a line the report should carry"),
        "{report}"
    );
}
