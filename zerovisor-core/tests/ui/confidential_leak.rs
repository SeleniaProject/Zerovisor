use zerovisor_core::info_flow::*;

fn main() {
    let secret = Labeled::<&str, Confidential>::new("secret");
    // This should not compile because leaking confidential data.
    log_public(&secret);
} 