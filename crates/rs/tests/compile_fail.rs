//! What must *not* compile.
//!
//! The compile-time guarantee is the product, so every rejection it promises
//! gets a case here. `trybuild` compiles each `tests/ui/*.rs` and compares the
//! compiler's output against the matching `.stderr`.
//!
//! # Two sets, because they are two different things
//!
//! `tests/ui/*.rs` are cases whose error text is **ours** — the `query!`
//! macro's own `compile_error!` strings, and the
//! `#[diagnostic::on_unimplemented]` message on the `fetch_all` bound. Those
//! read the same on every toolchain, so they run always.
//!
//! `tests/ui/toolchain/*.rs` are cases whose error text is **rustc's**, and
//! rustc rewords its diagnostics between releases. `wrong_param_type` is
//! rejected by a plain `V: Into<T>` bound, so its `.stderr` is a snapshot of
//! the compiler's help for an unsatisfied `From` bound. That help changed
//! shape between 1.95, which lists a few impls and then says "and N others",
//! and 1.98, which prints the list in full under a differently worded lead
//! line — neither of which is ours to promise. It is also a snapshot of the
//! *dependency graph*: the impl list names `deranged::RangedI64` and
//! `zerocopy::byteorder::I64`, so a dependency bump can move it too.
//!
//! Left in the default set, that snapshot took the whole workspace suite down
//! with it: `cargo test --workspace` is fail-fast across targets, so a
//! contributor on a different stable release lost every test after this one —
//! which is exactly how a real regression hides. It now runs only when
//! `SURREALQL_ANALYZER_UI_TOOLCHAIN=1` is set. CI sets it, so the rejection is
//! still enforced on a known toolchain; bless it there with
//! `TRYBUILD=overwrite`.
//!
//! The durable fix is to stop borrowing rustc's prose for this case at all —
//! give the parameter-binding bound its own
//! `#[diagnostic::on_unimplemented]`, the way the `fetch_all` bound already
//! has one, and the case moves back to the always-on set. That is a change to
//! the macro's public bound, so it is not bundled here.

// The `.stderr` files record one compiler's exact wording, and rustc rewords
// its diagnostics between channels as well as between releases — `From`
// currently prints its impl list one way on stable and another on nightly, so
// the same file cannot match both. They are recorded on stable, which is what
// CI builds with. Bless with `TRYBUILD=overwrite` on stable.
#[rustversion::attr(not(stable), ignore = "stderr is recorded on stable")]
#[test]
fn rejections_are_compile_errors() {
    // The macro resolves the schema from `CARGO_MANIFEST_DIR`, which trybuild
    // repoints at its own generated crate — so name the schema explicitly.
    let schema = concat!(env!("CARGO_MANIFEST_DIR"), "/schema.surql");
    // SAFETY: single-threaded test setup, before any thread is spawned.
    unsafe { std::env::set_var("SURREALQL_ANALYZER_SCHEMA", schema) };

    let cases = trybuild::TestCases::new();
    cases.compile_fail("tests/ui/*.rs");
    // Opt-in: see the module docs. Any value enables it; unset skips it.
    if std::env::var_os("SURREALQL_ANALYZER_UI_TOOLCHAIN").is_some() {
        cases.compile_fail("tests/ui/toolchain/*.rs");
    }
}
