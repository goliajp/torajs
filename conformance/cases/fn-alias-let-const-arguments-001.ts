// A `let` / `const` alias of a function whose body reads `arguments`.
// The alias stays a direct FnSig call by design, but such a body's
// signature carries a hidden argument-count slot (and an argv pointer
// on the value face) that the FnSig lane cannot see: `const c = E.one;
// c(true)` fed `true` into the count and read `a` off nothing, and a
// zero-parameter static read its count off a stack pointer. The alias
// now takes the relay that `var c = E.one` already took, and the fold
// back to direct calls keeps every all-call alias on the direct lane.
// Static methods with a boolean slot (twin), untyped and zero-param
// statics, top-level functions, a spread call through the alias, and
// an alias that is also read as a value, and an alias whose target is
// called twice with no other use of it (the folded relay is dead and
// must not be left calling a generic template).
class E {
  static one(a: boolean) { return [a, arguments.length]; }
  static plain(a, b) { return [a, b, arguments.length]; }
  static zero() { return arguments.length; }
  static n(a: number) { return [a, arguments.length]; }
}
function f(a: boolean) { return [a, arguments.length]; }
function g(a, b) { return [a, b, arguments.length]; }
function z() { return arguments.length; }
const arr = [1, 2];
const c = E.one;
console.log(JSON.stringify(c(true)), JSON.stringify(c(false, 1)));
let l = E.one;
console.log(JSON.stringify(l(true)), JSON.stringify(l(false, 1)));
var v = E.one;
console.log(JSON.stringify(v(true)), JSON.stringify(v(false, 1)));
const p = E.plain;
console.log(JSON.stringify(p(1)), JSON.stringify(p(1, 2, 3)), JSON.stringify(p(1, 2, ...arr)));
const zz = E.zero;
console.log(zz(), zz(1, 2));
const nn = E.n;
console.log(JSON.stringify(nn(1)), JSON.stringify(nn(1, 2)));
const cf = f;
console.log(JSON.stringify(cf(true)), JSON.stringify(cf(false, 1)));
const cg = g;
console.log(JSON.stringify(cg(1)), JSON.stringify(cg(1, 2, 3)), JSON.stringify(cg(1, ...arr)));
let lz = z;
console.log(lz(), lz(1, 2));
const both = E.plain;
console.log(JSON.stringify(both(4)), typeof both, [both].length);
function twice(a, b) { return arguments.length; }
const tw = twice;
console.log(tw(1), tw(1, 2));
var tv = twice;
console.log(tv(1, 2), tv(3, 4));
