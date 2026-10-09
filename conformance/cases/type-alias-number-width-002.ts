// A name that something other than one top-level alias claims keeps
// resolving by scope: a type parameter and an enum spelled like an
// alias each mean their own type where they are in scope.

type S = string;
function id<S>(x: S): S {
  return x;
}
console.log(id(2.5), id("s"));

enum E {
  A = 1,
  B = 2,
}
const e: E = E.B;
console.log(e, E.A);
