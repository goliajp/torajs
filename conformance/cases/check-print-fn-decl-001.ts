// fn-name registry Phase 2 Step 6 — top-level `function <name>()`
// declaration round-trips through the `__torajs_fn_name_table[]`
// rodata + `__torajs_fn_print_inline` lookup, producing bun's
// `[Function: <name>]\n` form exactly. ssa_lower Pass 2's fn-decl
// walk picks up both `foo` and `bar`; torajs-link's Step 3b.4-5
// chain-fixup plants the fn body / name bytes vaddrs into the
// rodata entries; the runtime helper walks the entries linearly.
function foo() {
    return 1;
}
function bar() {
    return 2;
}
console.log(foo);
console.log(bar);
