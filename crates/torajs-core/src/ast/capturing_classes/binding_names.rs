//! The spellings of the two class bindings the lane mints for a
//! class (§14.2.3), as other passes ask about them.

/// Is this a CLASS BINDING this lane mints — the container-facing
/// `__cc<N>_<user name>` or the class-scope `__cci<N>_<user name>`
/// §14.2.3 gives it a second one of (see `own_binding`)?
///
/// The lane's other minted spellings all put a letter where these put
/// a digit (`__cca<N>_` aliases, `__ccm_<n>__` member sentinels,
/// `__ccmk_<C>_<n>` computed keys, `__ccp<N>` heritage bindings,
/// `__ccn<N>_` construct twins), so
/// the digit is what tells them apart — the inner one wears its `i`
/// ahead of the digits for the same reason.
///
/// Asked outside the AST passes by the top-level data-global gate:
/// a desugar-minted `__`-prefixed name stays a main-local there, but
/// this one is a USER class name in disguise — α-renamed, holding the
/// ES5 constructor function — and a named function reading the class
/// it was declared next to has to find it. Without the carve-out
/// `function g() { return K; }` answered `unknown ident __cc0_K`, and
/// the `typeof` spelling of the same read answered `"undefined"`
/// (§13.5.3's answer for an unresolvable reference).
pub(crate) fn is_es5_class_binding(name: &str) -> bool {
    is_es5_inner_class_binding(name) || numbered_binding(name, "__cc")
}

/// Is this the CLASS-SCOPE half of the pair — `__cci<N>_<user name>`?
///
/// Only the class's own bodies can see it: the lane mints it exactly
/// when the body reads its own name, and the container reads the
/// mutable `__cc<N>_` alias instead. So a WRITE to this name is
/// §15.7.14 step 3's immutable binding being written from inside the
/// class, which is a runtime TypeError rather than anything the
/// checker should refuse (`check_assign_ident`) — and the `const`
/// the lane declares does not catch it, because a method body reaches
/// the binding as a lifted-closure capture.
pub(crate) fn is_es5_inner_class_binding(name: &str) -> bool {
    numbered_binding(name, "__cci")
}

/// `<prefix><digits>_…` — the shape both halves of the pair wear, and
/// the digit is what separates them from the lane's letter-carrying
/// spellings.
fn numbered_binding(name: &str, prefix: &str) -> bool {
    let Some(rest) = name.strip_prefix(prefix) else {
        return false;
    };
    let digits = rest.len() - rest.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    digits > 0 && rest[digits..].starts_with('_')
}
