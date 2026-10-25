//! Command-line cases, declared as files in `tests/cmd/`.

#[test]
fn cli_cases() {
    trycmd::TestCases::new().case("tests/cmd/*.toml");
}
