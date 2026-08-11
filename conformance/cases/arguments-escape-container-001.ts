// rotation 362 — container-stored fn-exprs with arguments VALUE
// reads: an array-literal element position is boxed-only consumption,
// so the binding chain admits the fn to the argv face, and a
// rest-tail element callee dispatches through the boxed dual entry.
const f = function () {
  return arguments[0];
};
const box = [f];
console.log(box[0](42)); // element-call lane

const g = function () {
  return arguments.length + (arguments[0] ?? 0);
};
const keep = [g];
console.log(g(5)); // direct lane (argv fed at the call site)
console.log(keep[0](7)); // adapter lane, same fn

const h = function () {
  return arguments[0];
};
const hbox = [h];
const alias = hbox[0];
console.log(alias(11)); // alias-of-element (variadic local)

const m = function () {
  return arguments.length + (arguments[1] ?? 0);
};
const mixed = [m, (n: number) => n + 1];
console.log(mixed[0](1, 2)); // mixed-element array, argv fn slot
console.log(mixed[1](5)); // plain closure slot unaffected

const s = function () {
  return arguments[0];
};
const store = [s];
console.log(typeof store[0]); // store-only element read, no call
