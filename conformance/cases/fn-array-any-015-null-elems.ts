// An element of `(F | null)[]` (held boxed) bound to an `F | null`
// binding, assigned to one, or handed to an `F | null` parameter: `null`
// stays `null`, a function calls.
type F = (x: number) => number;
const f: F = (x) => x + 1;
const hs: (F | null)[] = [null, f];

const b: F | null = hs[0];
console.log(b === null, b === undefined, typeof b);
if (b) console.log(b(1));
else console.log("null");

let c: F | null = hs[1];
console.log(c!(2));
c = hs[0];
console.log(c === null, c ? c(3) : "null");
c = hs[1];
console.log(c ? c(3) : "null");

function run(k: F | null) {
  return k === null ? "null" : k(4);
}
console.log(run(hs[0]), run(hs[1]), run(hs.at(0) ?? null));
