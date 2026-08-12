// §10.2.1.2 step 6 on the PLAIN-function lane: a function declaration
// that merely mentions `this` gets its `__this` parameter from
// `bind_this_param`, whose direct-call seed is `undefined` — the
// strict answer. Under the sloppy script goal the same callee-side
// prologue the promoted bodies carry makes a receiverless call see
// the global object instead.
//
// The strict half of this rule (own directive, and the lexical
// inheritance the parser writes in) deliberately gets no case here:
// bun answers `object` for both in a .cts, its transpile layer having
// dropped the per-function directive, so a bun-parity fixture would
// pin the divergence rather than the spec.
function readsThis() {
  return this === globalThis;
}
console.log(readsThis());

function writesThis() {
  (this as any).__plain_p = 7;
}
writesThis();
console.log((globalThis as any).__plain_p);

// A receiver that IS supplied is untouched — the prologue only fills
// the undefined/null slot.
function tagged() {
  return (this as any).tag;
}
console.log(tagged.call({ tag: "kept" }));
