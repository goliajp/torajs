//! Re-export the runtime staticlib paths torajs-core's build script
//! publishes (`DEP_TORAJS_RUNTIME_<NAME>_STATICLIB_PATH`) as rustc env
//! vars of this crate, under the names `runtime_staticlib_envs!` lists,
//! so `runtime_linked_image!` reads the same archives the runtime image
//! in torajs-core was built from.

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    // Opt-in flag the census build sets (RUSTFLAGS=--cfg=torajs_census);
    // this crate has no [lints] table, so it declares the name itself.
    println!("cargo::rustc-check-cfg=cfg(torajs_census)");
    let mut n = 0;
    for (key, path) in std::env::vars() {
        if let Some(name) = key.strip_prefix("DEP_TORAJS_RUNTIME_") {
            println!("cargo::rustc-env={name}={path}");
            n += 1;
        }
    }
    assert!(n > 0, "torajs-core published no staticlib paths");
}
