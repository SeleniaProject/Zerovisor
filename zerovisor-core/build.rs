// build.rs for zerovisor-core
// Automatically links the Coq extracted proof library when the `coq_proofs` feature is enabled.
// The build script is intentionally lightweight: it only injects `cargo:` directives so that
// upstream tooling (e.g. Bazel, cargo-make, or the monorepo CI) can decide how to provide the
// actual static library.  We avoid spawning expensive build steps here because the proofs are
// assumed to be pre-compiled by the formal verification pipeline.
//
// Environment Variables
// ---------------------
// • COQ_PROOFS_LIB_DIR — Optional.  If set, the path is added to `rustc`’s library search path.
// • COQ_PROOFS_LIB_NAME — Optional.  Defaults to `coq_verifications`.
//
// If the `coq_proofs` feature flag is *not* enabled this script is effectively a no-op.

fn main() {
    // Cargo automatically sets the environment variable `CARGO_FEATURE_<name>` for enabled
    // feature flags.  We leverage that to determine whether we need to link against the
    // Coq proof archive.
    if std::env::var("CARGO_FEATURE_COQ_PROOFS").is_ok() {
        // Pass user-supplied search path to rustc.
        if let Ok(dir) = std::env::var("COQ_PROOFS_LIB_DIR") {
            println!("cargo:rustc-link-search=native={}", dir);
        }

        // Use a configurable library name but default to the canonical one used by our
        // formal verification pipeline.
        let lib_name = std::env::var("COQ_PROOFS_LIB_NAME").unwrap_or_else(|_| "coq_verifications".into());
        println!("cargo:rustc-link-lib=static={}", lib_name);

        // Ensure downstream crates rerun the build script if these variables change.
        println!("cargo:rerun-if-env-changed=COQ_PROOFS_LIB_DIR");
        println!("cargo:rerun-if-env-changed=COQ_PROOFS_LIB_NAME");
    }
} 