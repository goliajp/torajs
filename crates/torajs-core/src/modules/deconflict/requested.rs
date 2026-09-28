//! A requested spelling the census has met before: does the earlier
//! visit's mangle still bind it, or does the request land as asked?

use std::collections::HashMap;

use super::SeedNames;

/// The spelling asked for is the one this decl itself already landed
/// under: the request binds to that binding
/// (`repeat_request::bind_repeat_decls`), nothing collides. Errs when
/// it landed under a mangle and the spelling asked for is held by some
/// other declaration — the program's own duplicate binding, kept loud.
pub(super) fn settle_requested_landing(
    prior: &mut HashMap<String, String>,
    landed: &HashMap<String, String>,
    seed: &SeedNames,
    name: &str,
    surface: &str,
) -> Result<(), String> {
    if !prior.contains_key(name) {
        return Ok(());
    }
    match landed.get(name).map(String::as_str) {
        Some(l) if l != surface && seed.hard.contains(surface) => Err(format!(
            "import name `{surface}` collides with a same-named export of \
             another module; import it through a namespace (`import * as \
             ns`) instead"
        )),
        // Landed under a mangle that only a RESERVATION forced — the
        // entry imports this spelling from a path that re-exports it
        // from here — and nothing holds the spelling: the request
        // binds to that landing (`repeat_request::bind_repeat_decls`).
        Some(_) => Ok(()),
        // Mangled by an earlier walk that never injected it, for the
        // same reservation: nothing holds either spelling, so the
        // request lands as asked and the mangle memory is void.
        None => {
            prior.remove(name);
            Ok(())
        }
    }
}
