//! The live official index, checked by the scheduled `official-index` CI job
//! (`cargo test -p engine --test integration official -- --ignored`) with the
//! keys compiled into this build, so a rotation that outruns a release fails
//! there.

use engine::verify_index;

const WARN_WITHIN_SECONDS: i64 = 10 * 24 * 60 * 60;

fn read(name: &str) -> Vec<u8> {
    let dir =
        std::env::var("OFFICIAL_INDEX_DIR").expect("OFFICIAL_INDEX_DIR names the fetched files");
    std::fs::read(std::path::Path::new(&dir).join(name)).unwrap()
}

#[test]
#[ignore = "reads the files the CI job fetched"]
fn the_published_official_index_verifies_and_is_not_about_to_expire() {
    let bytes = read("plugins.json");
    let signature = String::from_utf8(read("plugins.json.minisig")).unwrap();
    let keys: Vec<String> = common::app::official_plugin_pubkeys()
        .into_iter()
        .map(str::to_string)
        .collect();
    let now = common::now_unix();

    let index = verify_index(&bytes, &signature, &keys, "official", None, now).unwrap();

    assert!(
        index.expires - now > WARN_WITHIN_SECONDS,
        "the official index expires in under 10 days"
    );
}
