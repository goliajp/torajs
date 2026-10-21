//! The class a sid's cells are the instances of, by the name its
//! methods are lowered under.

use std::collections::HashMap;

/// Each sid one class alone reaches, with that class's name; a sid two
/// classes share is left out.
pub(crate) fn sole_names<'a>(
    classes: impl IntoIterator<Item = (&'a str, u32)>,
) -> HashMap<u32, String> {
    let mut by_sid: HashMap<u32, Option<&str>> = HashMap::new();
    for (name, sid) in classes {
        by_sid
            .entry(sid)
            .and_modify(|n| *n = None)
            .or_insert(Some(name));
    }
    (by_sid.into_iter())
        .filter_map(|(sid, n)| Some((sid, n?.to_string())))
        .collect()
}
