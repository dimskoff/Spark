//! Compile-fail tests: wrong attributes must give a clear error at the attribute.

#[test]
fn ui() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/*.rs");
}
