// An arrow function that runs off the end of its body answers
// `undefined` (ES §10.2.1.4 step 11), the same as a named one.
//
// The lowering already put the right sentinel in the return slot — an
// arrow is lifted to a `__closure_N` FnDecl before the width analysis
// runs, so it lands on the fall-through table under that synthetic
// name. The call site spells it with the binding it was assigned to,
// so the consuming end looked the name up and missed: a `number` arrow
// answered NaN. The binding is now recorded as an alias of the lifted
// name.

const num = (f: boolean): number => {
  if (f) return 7;
};

const str = (f: boolean): string => {
  if (f) return "s";
};

const obj = (f: boolean) => {
  if (f) return { v: 1 };
};

// no annotation at all — implicit-generics fills one in
const inferred = (f) => {
  if (f) return 7;
};

console.log(num(false), num(true));
console.log(str(false), str(true));
console.log(obj(false), obj(true));
console.log(inferred(false), inferred(true));

console.log(typeof num(false));
console.log(num(false) === undefined);
console.log(typeof str(false));
console.log(str(false) === undefined);

// a real 0 from the same arrow is still a 0
const zero = (f: boolean): number => {
  if (f) return 7;
  return 0;
};
console.log(zero(false), zero(false) === undefined);

// declared inside a function body, not at the top level
function outer() {
  const inner = (f) => {
    if (f) return 3;
  };
  console.log(inner(false), inner(true));
}
outer();
