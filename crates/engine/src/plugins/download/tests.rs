use proto::download::HashAlgorithm;
use tokio::sync::watch;

use super::*;
use crate::cancel::Cancel;

fn checksum(algorithm: HashAlgorithm, hex: &str) -> Checksum {
    Checksum {
        algorithm,
        hex: hex.into(),
    }
}

#[test]
fn a_checksum_is_checked_for_length_and_folded_to_lowercase() {
    let sha1 = "ABCDEF0123456789ABCDEF0123456789ABCDEF01";
    let folded = normalised(checksum(HashAlgorithm::Sha1, sha1)).unwrap();
    assert_eq!(folded.hex, sha1.to_ascii_lowercase());

    for bad in [
        checksum(HashAlgorithm::Sha1, "abc"),
        checksum(HashAlgorithm::Sha256, &"a".repeat(40)),
        checksum(HashAlgorithm::Sha512, &"z".repeat(128)),
    ] {
        assert!(matches!(normalised(bad), Err(GuestError::Invalid(_))));
    }
    assert!(normalised(checksum(HashAlgorithm::Sha512, &"a".repeat(128))).is_ok());
}

#[test]
fn a_logged_place_is_host_and_path_without_the_query() {
    assert_eq!(
        place("https://files.example.com/a/b.jar?token=secret#x"),
        "files.example.com/a/b.jar"
    );
    assert_eq!(place("not a url"), "");
}

fn run() -> (CommandRun, watch::Receiver<Option<PluginRunProgress>>) {
    let (progress, reports) = watch::channel(None);
    let run = CommandRun {
        cancel: Cancel::new(),
        progress,
        grace: Duration::from_secs(1),
    };
    (run, reports)
}

#[test]
fn progress_is_reported_once_per_thousandth_and_not_at_all_without_a_run() {
    let (run, mut reports) = run();
    let reporter = Reporter::new(Some(&run), "tool.jar");
    let mut changes = 0;
    for downloaded in 0..=1_000_000u64 {
        if downloaded % 100 != 0 {
            continue;
        }
        reporter.report(&DownloadProgress {
            downloaded,
            total: 1_000_000,
        });
        if reports.has_changed().unwrap() {
            reports.borrow_and_update();
            changes += 1;
        }
    }
    assert_eq!(changes, 1001);

    Reporter::new(None, "x").report(&DownloadProgress {
        downloaded: 1,
        total: 2,
    });
}

#[test]
fn an_unknown_size_reports_without_a_fraction() {
    let (run, reports) = run();
    let reporter = Reporter::new(Some(&run), "tool.jar");
    reporter.report(&DownloadProgress {
        downloaded: 3 << 20,
        total: 0,
    });
    let latest = reports.borrow().clone().unwrap();
    assert_eq!(latest.fraction, None);
    assert_eq!(latest.message, "downloading tool.jar");
}
