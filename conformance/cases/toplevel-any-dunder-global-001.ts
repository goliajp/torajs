// A top-level `any` binding read from a NAMED function body promotes
// to a data global; a local slot would not be visible there. The gate
// deciding that used to skip every `__`-prefixed name as a minted
// sentinel, so these five programs were `ssa-lower: unknown ident` —
// each identical to a working one but for the two leading underscores.

let __v: any = 1;
function inc() {
  __v = __v + 1;
}
inc();
inc();
console.log(__v);

let __o: any = { n: 0 };
function bumpMember() {
  __o.n = __o.n + 1;
}
bumpMember();
console.log(__o.n);

let __s: any = "s";
function read() {
  return __s;
}
console.log(read(), typeof read());

// the same binding written from one named fn and read from another
let __shared: any = "a";
function append() {
  __shared = __shared + "b";
}
function get() {
  return __shared;
}
append();
append();
console.log(get());

// and a name the compiler DOES mint stays out of the promote lane —
// `x in o` becomes a synthetic `__torajs_in_op` call at parse time,
// which still has to resolve as an intrinsic rather than a global
const probe = { a: 1 };
console.log("a" in probe, "b" in probe);
