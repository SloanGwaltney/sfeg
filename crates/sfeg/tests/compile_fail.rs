//! Mistakes that must not compile, each checked against the error it
//! gives. The cases are in `tests/compile_fail/`.

#[test]
fn compile_errors_explain_the_mistake() {
    trybuild::TestCases::new().compile_fail("tests/compile_fail/*.rs");
}
