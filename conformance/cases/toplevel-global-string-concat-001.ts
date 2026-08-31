// A top-level binding is only visible from a named fn body when it
// promotes to a data global, and promotion needs the slot's runtime
// type to be statically certain. A concatenation was not on that
// list, so `const src = "a b" + "!"` stayed a main-fn local and every
// named-fn read of it threw ReferenceError — while the same program
// with the halves already joined compiled and ran.
const src = "a b" + "!";
function words(): number {
  return src.split(" ").length;
}
console.log(words(), src, src.length);

// through an aliased operand, and folded left-to-right
const pre = "x";
const two = pre + "y" + "z";
function readTwo(): string {
  return two;
}
console.log(readTwo(), two.length);

// one side is enough — §13.15.3 concatenates whenever either
// primitive is a string, whichever side it is on
const n1: number = 3;
const numRight = "n" + n1;
const numLeft = n1 + "s";
const b1: boolean = true;
const withBool = "b" + b1;
function readAll(): string {
  return numRight + "|" + numLeft + "|" + withBool;
}
console.log(readAll(), numRight.length);

// a write from a named fn body lands in the same slot the main side
// reads, as it does for a plain string literal binding
let mutable = "p" + "q";
function bump(): void {
  mutable = mutable + "!";
}
bump();
bump();
console.log(mutable);
