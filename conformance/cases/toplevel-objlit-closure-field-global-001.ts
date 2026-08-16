// A top-level object literal with a method-valued field, read by a
// named function. The un-annotated slot spelling admitted only
// number / string / boolean literal fields, so one closure field kept
// the whole binding main-local and every named fn saw "unknown
// identifier" — even for the plain number field beside it.
const api = {
  base: 2,
  label: "api",
  on: true,
  twice: (x: number) => x * 2,
  greet: (who: string) => "hi " + who,
};

function readPlain() {
  return api.base + api.label.length + (api.on ? 1 : 0);
}
function callField() {
  return api.twice(21);
}
function readAsValue() {
  return typeof api.twice;
}

console.log("plain", readPlain());
console.log("call", callField());
console.log("value", readAsValue());
console.log("greet", api.greet("you"));

// a closure (not just a named fn) reaching the same binding
const viaClosure = () => api.twice(api.base);
console.log("closure", viaClosure());

// borrowing the field out and calling it
function borrow() {
  const f = api.twice;
  return f(4);
}
console.log("borrow", borrow());

// the written annotation names the same slot
const typed: { twice: (x: number) => number } = { twice: (x: number) => x * 2 };
function callTyped() {
  return typed.twice(5);
}
console.log("typed", callTyped());
