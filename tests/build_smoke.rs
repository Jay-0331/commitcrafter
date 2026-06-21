#[test]
fn library_loads_and_reports_version() {
    let v = commitcrafter::version();
    assert!(!v.is_empty(), "version string is empty");
    assert!(
        v.chars().any(|c| c.is_ascii_digit()),
        "version has no digits: {v}"
    );
}
