// chunk 793 — fn-typed fields of INLINE object type annotations are
// Closure-repr, same as named-TypeDecl fields. The parser now retags
// `__fn(` → `__cls(` at the `__inlobj(` birth site (previously only
// tag_struct_field_closure_types covered TypeDecl/ClassDecl, so an
// inline-ann field parsed to FnSig while the literal stored a closure
// env block — the field call CallIndirect'd into the env header,
// SIGBUS). The named-fn forwarder wrap axes (let-init / call-arg /
// member-assign) resolve inline object annotations uniformly with
// TypeDecl names; the call-arg objlit axis is new for BOTH spellings.
function topfn(): number {
  return 11;
}
function two(): number {
  return 22;
}

// arrow via typed binding crossing a param boundary (was SIGBUS)
function take(o: { fn: () => number }): number {
  return o.fn();
}
const t = { fn: () => 7 };
console.log(take(t));
console.log(take({ fn: () => 8 }));

// named-fn store at let-init (was SIGBUS)
const a: { k: () => number } = { k: topfn };
console.log(a.k());

// capturing arrow in inline-ann field
let outer = 5;
const b: { tick: () => number } = { tick: () => outer + 1 };
console.log(b.tick());

// return-position inline ann
function mk(): { fn: () => number } {
  return { fn: () => 3 };
}
console.log(mk().fn());

// named-fn store at call-arg objlit — inline ann (was SIGBUS)
function take3(o: { k: () => number }): number {
  return o.k();
}
console.log(take3({ k: topfn }));

// named-fn store at call-arg objlit — named TypeDecl (was SIGBUS,
// pre-existing: the call-arg objlit wrap axis did not exist)
type O = { k: () => number };
function take4(o: O): number {
  return o.k();
}
console.log(take4({ k: topfn }));

// member-assign named-fn into inline-ann field (was SIGBUS)
const c: { k: () => number } = { k: topfn };
c.k = two;
console.log(c.k());
