use engr::store::run_conformance_dir;
use std::path::PathBuf;

#[test]
fn all_protocol_v1_fixtures_pass() {
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("conformance/fixtures");
    run_conformance_dir(&fixtures).expect("the immutable protocol-v1 fixture corpus must pass");
}
