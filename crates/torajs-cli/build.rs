/* Stamp the binary with a build-time epoch so `tr run`'s cache
 * key invalidates cleanly when the compiler is rebuilt — without
 * this, a freshly-built `tr` would happily exec an older cached
 * binary compiled by a buggy version of itself, which once hid a
 * dispatch-tag-collision fix for a full session. */
fn main() {
    let epoch = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    println!("cargo:rustc-env=TORAJS_BUILD_EPOCH={epoch}");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src");
}
