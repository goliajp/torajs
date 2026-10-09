// A top-level function named like a global function property
// (§19.2, §B.2.1) is the binding a bare call of that name reaches:
// the global environment's declarative record answers before the
// global object (§9.1.1.1, §9.4.2).

function escape(cr: number, ci: number, maxIter: number): number {
  let n = 0;
  let x = cr;
  while (n < maxIter && x < 2) {
    x = x + 1 + ci * 0;
    n++;
  }
  return n;
}
console.log(escape(-1.5, -1, 60));

function parseInt(s: string): number {
  return s.length * 100;
}
function isNaN(x: number): string {
  return "mine" + x;
}
function encodeURI(x: string): number {
  return x.length + 7;
}
console.log(parseInt("12"), isNaN(3), encodeURI("a b"));

// a call that precedes the declaration still reaches it (hoisted)
function late(): number {
  return unescape(3);
}
function unescape(n: number): number {
  return n * 10;
}
console.log(late());

// the names nobody rebinds keep the builtin
console.log(parseFloat("2.5"), isFinite(1), decodeURIComponent("%41"));
const h = escape;
console.log(typeof escape, h(0, 0, 9), escape.length);

// the global object's own property is still the builtin
console.log(globalThis.escape("a b"), globalThis.parseInt("12"));
