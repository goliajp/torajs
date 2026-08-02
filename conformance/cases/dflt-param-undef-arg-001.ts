// L3b #11 — explicit `undefined` argument triggers the default (§10.2.1.4
// IsUndefined): the callee-side guard, not the call-site pad, must observe it.
function g(a, b = 39) {
  return a + b;
}
console.log(g(1, undefined));
console.log(g(1, 2));
console.log(g(1));

function h(a, b = 39,) {
  return a + b;
}
console.log(h(1, undefined));

const arrow = (a, b = 7) => a + b;
console.log(arrow(3, undefined));
console.log(arrow(3, 4));

const fexpr = function fe(a, b = 11) {
  return a + b;
};
console.log(fexpr(5, undefined));

// mid-position undefined with a real trailing argument
function m(a, b = 39, c = 1) {
  return a + b + c;
}
console.log(m(42, undefined, 1));

async function f(a, b = 39,) {
  return a + b;
}
const afe = async function af2(a, b = 39, c = 1) {
  return a + b + c;
};
const aar = async (a, b = 39, c = 1) => a + b + c;
async function* ag(a, b = 39, c = 1) {
  yield a + b + c;
}
async function main() {
  console.log(await f(42, undefined));
  console.log(await afe(42, undefined, 1));
  console.log(await aar(42, undefined, 1));
  for await (const v of ag(42, undefined, 1)) console.log(v);
}
main();
