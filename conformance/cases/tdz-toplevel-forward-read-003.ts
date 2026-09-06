// §8.1.1.1, third half — the dead-zone read whose binding has no
// literal initializer at all. Nothing gives such a binding a home a
// read can reach, so the read used to resolve NOWHERE: the checker
// filed it as an unresolvable Reference (a spurious `unknown
// identifier` warning for a name the program declares, plus a second
// raise with the wrong message) and the lowerer aborted the build.
// The binding does exist; it is uninitialized, which is a different
// thing, and the raise already emitted at the read says so.
function seven(): number {
  return 7;
}
const prefix = "x";

try {
  console.log(sum);
} catch (e) {
  console.log((e as Error).message);
}
const sum = 2 + 3;
console.log(sum);

try {
  console.log(neg);
} catch (e) {
  console.log((e as Error).message);
}
const neg = -5;
console.log(neg);

try {
  console.log(fromCall);
} catch (e) {
  console.log((e as Error).message);
}
const fromCall = seven();
console.log(fromCall);

try {
  console.log(shape);
} catch (e) {
  console.log((e as Error).message);
}
const shape = { v: 1 };
console.log(shape.v);

try {
  console.log(items);
} catch (e) {
  console.log((e as Error).message);
}
const items = [1, 2];
console.log(items.length);

try {
  console.log(joined);
} catch (e) {
  console.log((e as Error).message);
}
const joined = prefix + "y";
console.log(joined);

// An identifier that really is not declared keeps the other message.
try {
  console.log(missing);
} catch (e) {
  console.log((e as Error).message);
}
