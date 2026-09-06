// §8.1.1.1 — a top-level let / const read written earlier in the
// statement list than its declaration is in the binding's temporal
// dead zone, so evaluating it raises ReferenceError. tr used to
// answer the literal instead, silently.
try {
  console.log(a);
} catch (e) {
  console.log(e instanceof ReferenceError, (e as Error).message);
}
const a = 5;
console.log(a);

try {
  console.log(flag);
} catch (e) {
  console.log((e as Error).message);
}
const flag = true;

// typeof answers "undefined" for an UNRESOLVABLE name, but a
// dead-zone binding resolves — GetValue on it throws.
try {
  console.log(typeof width);
} catch (e) {
  console.log((e as Error).message);
}
const width = 12;
console.log(typeof width);

// Nested control flow inside one top-level statement is still before
// the later declaration.
if (a === 5) {
  try {
    console.log(depth);
  } catch (e) {
    console.log((e as Error).message);
  }
}
const depth = 3;

// A named fn written BEFORE the declaration reads it when CALLED,
// which is after — legal, and must keep answering.
function later(): number {
  return tail * 2;
}
const tail = 21;
console.log(later());

// An inner binding of the same name is what the read sees.
{
  const depth2 = 1;
  console.log(depth2);
}
const depth2 = 9;
console.log(depth2);

// `var` has no dead zone — it initializes to undefined at hoist.
console.log(typeof legacy);
var legacy = 4;
