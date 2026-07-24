// K.6 follow-up (RFC 20260725 rotation 206) — mutable struct-typed
// top-level bindings promote to module globals: named fns see field
// writes and whole-binding reassignment.
type P = { a: number, b: string };

let s: P = { a: 1, b: "x" };

function bump() {
  s.a = s.a + 10;
}
function readBoth(): string {
  return s.a + ":" + s.b;
}

console.log(readBoth());
bump();
console.log(readBoth());

// whole-binding reassignment from main — old cell drops, named fns
// see the fresh cell
s = { a: 100, b: "y" };
console.log(readBoth());

// reassignment from inside a named fn
function swap() {
  s = { a: 7, b: "z" };
}
swap();
console.log(readBoth());
bump();
console.log(s.a);

// arrow capture reads the same shared binding
const peek = () => s.a;
s.a = 55;
console.log(peek());

// const struct global stays working alongside
const t: P = { a: 2, b: "c" };
function touchT() {
  t.a = 3;
}
touchT();
console.log(t.a);
