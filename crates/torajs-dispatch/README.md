# torajs-dispatch

The any-lane builtin-method dispatch seam. This crate exists to be
its own archive member: `libtorajs_anyvalue.a` calls
`__torajs_any_method_dispatch` through an extern declaration (a true
undefined reloc), this member provides the default definition
forwarding to the monolithic dispatcher, and a compiler-emitted
specialized dispatcher in the user `.o` shadows it at link time —
the monolith then loses its last reference and dead-strips.

See `.claude/rfcs/20260824-s2-5-selective-registration` (Phase B).
