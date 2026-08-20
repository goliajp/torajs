// An implicit-generic decl (any un-annotated param) has no lowered
// original — only per-call monomorphs. Its VALUE faces therefore have
// to ride the canonical forwarder cell: `typeof` used to answer
// `undefined` and an expando write died with `unknown ident`.
function ident(a) {
  return a;
}
ident.tag = "static";

console.log(typeof ident);
console.log(ident.name, ident.length);
console.log(ident.tag);

ident.tag = 42;
console.log(ident.tag);

// The cell is a singleton: two reads answer the same object, so the
// expando written above is visible through an alias.
var alias = ident;
console.log(alias === ident, alias.tag);

// The direct call still monomorphizes per call site.
console.log(ident(1), ident("s"), ident(true));

// Property read of a name the fn does not own.
console.log(ident.missing);

// A second implicit-generic decl, to check the wrap is per-name.
function pair(a, b) {
  return [a, b];
}
pair.kind = "pair";
console.log(typeof pair, pair.kind, pair.length);
console.log(pair(1, 2)[1]);

// `name` / `length` stay on their static arms even here: the shim has
// lost the default, so reading them off the cell would answer the
// shim's own arity instead of §20.2.4.1's count.
function withDflt(a, b = 39) {
  return a;
}
withDflt.tag = "d";
console.log(withDflt.length, withDflt.name, withDflt.tag, typeof withDflt);

function withRest(a, ...rest) {
  return rest.length;
}
withRest.tag = "r";
console.log(withRest.length, withRest.name, withRest.tag, withRest(1, 2, 3));
