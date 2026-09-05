// A `this`-using function declaration has ONE value. `bind_this_param`
// gives such a declaration a hidden leading receiver parameter and
// seeds `undefined` at its direct call sites; every other way of
// reaching it is a value use, and the value has to carry the same
// shape or the hidden slot eats the caller's first argument.
function t(a: any) {
  return "a=" + a + "/" + typeof (this as any);
}

// The value read through an alias is the same object as the value read
// through any other route, and the alias itself is one object.
const h = t;
const g = t;
console.log(h === t, h === g, h === [t][0]);

// The hidden receiver is not an argument and not a declared parameter.
console.log(h(7));
console.log(h.length, t.length, h.name);

// An argument reaching a fn-typed parameter — with and without a
// closure sharing the slot, because only the second shape ever marked
// the parameter.
function take(f: (x: any) => string) {
  return f(7);
}
console.log(take(t));
console.log(take((z: any) => "c=" + z), take(t));

// Through a container and back out again. Read into a binding first:
// calling `box[0](8)` in place is a method-shaped invocation, whose
// receiver is the array — a separate boundary from this one.
const box = [h];
const out = box[0];
console.log(box[0] === t, out(8), out.length);
