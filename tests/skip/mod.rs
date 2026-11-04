//! A test that can't run on this machine (no IPv6 loopback, no lsof) says why
//! and passes, so a laptop, a container or the Nix sandbox can run the suite.
//! CI sets `PROC_TESTS_MUST_RUN`, and there a skip fails instead: a runner that
//! lost what a test needs would otherwise turn it into a silent pass.

// Windows builds none of the tests that call it
#![allow(dead_code)]

/// Report that the test can't run here, then return so the test passes; with
/// `PROC_TESTS_MUST_RUN` set, fail it instead
pub fn skipped(why: &str) {
    assert!(
        std::env::var_os("PROC_TESTS_MUST_RUN").is_none(),
        "this test must run here (PROC_TESTS_MUST_RUN is set), but can't: {why}"
    );
    proc_cli::err_println!("skipped: {why}");
}
