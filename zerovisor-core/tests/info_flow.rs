//! Tests for Information-flow control utilities

extern crate std;
use zerovisor_core::info_flow::*;

#[test]
fn compile_fail_confidential_to_public() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/confidential_leak.rs");
}

#[test]
fn downgrade_allows_logging() {
    let secret = Labeled::<&str, Confidential>::new("classified");
    let token = obtain_token();
    let pub_msg = secret.downgrade(token);
    log_public(&pub_msg);
} 