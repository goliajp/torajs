// §13.3.6.2 with a base that is not a plain binding. 591 could only
// seed the receiver from an Ident base, because the element read
// re-lowered the base itself and only a slot load is safe to run
// twice. Every other spelling silently called with `this ===
// undefined` — `b[0][0]()` ran, and read the wrong receiver.
//
// The base is now lowered once and handed to the element read, so
// the shape of the base stops mattering. `makeRow` below is the
// witness that this did not turn into a second evaluation: a base
// with a side effect must still run exactly one time.
let width = function () {
  return (this as any).length;
};
let kind = function () {
  return typeof (this as any);
};

// Nested index: the receiver is the inner row, not the outer array.
const grid = [[width], [width, width], [width, width, width]];
console.log(grid[0][0](), grid[1][0](), grid[2][0]());

// Three deep, to show nothing is special about one level.
const deep = [[[width, width]]];
console.log(deep[0][0][0]());

// A member base.
const bag = { row: [width, width, width, width] };
console.log(bag.row[0]());

// A call base — evaluated once, and it is that call's result that
// becomes the receiver.
let calls = 0;
function makeRow() {
  calls++;
  return [width, width];
}
console.log(makeRow()[0](), calls);

// Identity, not merely "an object with a length".
let isRow = function () {
  return (this as any) === row;
};
const row = [isRow];
const rows = [row];
console.log(rows[0][0]());

// Detaching still drops the base (§10.2.1.2).
const holder = [[kind]];
const held = holder[0][0];
console.log(held());

// NOT asserted here, because bun 1.4.1 answers it wrongly: an
// ARRAY-LITERAL base — `[k][0]()` — is still a property Reference,
// so `this` is that array. bun says `undefined`; node 26.8.1 and tr
// both say `object`. Left out of the fixture so this file stays
// byte-equal against bun; registered in the handoff instead.
