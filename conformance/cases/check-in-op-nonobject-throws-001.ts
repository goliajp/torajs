// ES §13.10.1 step 5 — the rhs of `in` must be an Object; every
// other rhs is a TypeError, not a `false` answer. Pre-fix the Any
// kernel returned false for all of these, so a wrong operand read
// as "property absent" instead of surfacing the mistake.
function mk(): any {
  return undefined;
}
const u: any = mk();
try {
  console.log("q" in u);
} catch (e) {
  console.log("undefined throws");
}
const n: any = null;
try {
  console.log("q" in n);
} catch (e) {
  console.log("null throws");
}
const num: any = 42;
try {
  console.log("q" in num);
} catch (e) {
  console.log("number throws");
}
const b: any = true;
try {
  console.log("q" in b);
} catch (e) {
  console.log("boolean throws");
}
const short: any = "ab";
try {
  console.log("length" in short);
} catch (e) {
  console.log("short string throws");
}
const long: any = "abcdefghijklmnopqrstuvwxyz0123456789";
try {
  console.log("length" in long);
} catch (e) {
  console.log("heap string throws");
}
const big: any = 10n;
try {
  console.log("q" in big);
} catch (e) {
  console.log("bigint throws");
}
// A numeric key takes the sibling kernel; same rejection.
try {
  console.log(0 in u);
} catch (e) {
  console.log("numeric key on undefined throws");
}
// Objects still answer normally — the gate must not swallow them.
const o: any = { a: 1 };
console.log("a" in o, "z" in o);
const arr: any = [1, 2];
console.log(0 in arr, 5 in arr, "length" in arr);
// Closure own-props (`fn.tag = 7; "tag" in fn`) are a separate
// pre-existing gap — the kernel has no TAG_CLOSURE arm, so it
// answers false where bun says true. Tracked in plan-state L3b;
// deliberately not asserted here so this fixture stays about the
// non-Object rejection.
