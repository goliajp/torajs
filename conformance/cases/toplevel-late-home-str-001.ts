// A top-level binding whose initializer is a string or symbol the
// compiler only knows once it has typed it (a field read, an element,
// a character, a slice, a conditional, `typeof`) is read and written
// from function declarations. It used to stay local to the program
// body: `typeof` answered "undefined" and every other read threw a
// ReferenceError.
function show(): void {
  console.log(kind, field, deep, word, ch, tail, pick, made);
  console.log(typeof sym, String(sym), label);
}
function mutate(): void {
  label = label + "+";
  sym = Symbol.asyncIterator;
}

const src = "hello world";
const o = { a: "alpha", inner: { b: "beta" } };
const words = src.split(" ");
const kind = typeof o;
const field = o.a;
const deep = o.inner.b;
const word = words[0];
const ch = src[1];
const tail = src.slice(6);
const pick = words.length > 1 ? "many" : "one";
function mk(): { s: string } {
  return { s: "made" };
}
const made = mk().s;
let sym = Symbol.iterator;
let label = o.a;

show();
mutate();
mutate();
show();
console.log(field, label);
