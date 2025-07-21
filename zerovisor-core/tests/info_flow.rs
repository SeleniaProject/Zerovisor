//! Tests for Information-flow control utilities

extern crate std;
use zerovisor_core::info_flow::*;

#[test]
fn confidential_cannot_be_logged() {
    let secret = Labeled::new("top-secret", SecurityTag::Confidential);
    // Should panic when attempting to log confidential data
    let result = std::panic::catch_unwind(|| {
        log_public(&Labeled::new("top-secret", SecurityTag::Confidential));
    });
    assert!(result.is_err());
}

#[test]
fn downgrade_allows_logging() {
    let secret = Labeled::new("classified", SecurityTag::Confidential);
    let token = obtain_token();
    let pub_msg = downgrade(secret, token);
    log_public(&Labeled::new(pub_msg.into_inner(), SecurityTag::Public));
} 