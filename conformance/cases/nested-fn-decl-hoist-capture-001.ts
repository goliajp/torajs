// Function declarations nested in a function body that capture an outer
// local are rewritten into closure bindings. §10.2.11 binds every one of
// them before the body runs, so they must see each other whatever their
// order, and a call written above the declaration's text must find the
// function live.

// mutual recursion inside a function expression (an IIFE)
const g = (function () {
  let c = 0;
  function even(n: any): any { c += 1; return n === 0 ? c : odd(n - 1); }
  function odd(n: any): any { c += 1; return n === 0 ? -c : even(n - 1); }
  return even;
})();
console.log(g(3));

// three-way, capturing, inside a named function
function three(): number {
  let k = 0;
  function a(n: number): number { k += 1; return n <= 0 ? k : b(n - 1); }
  function b(n: number): number { k += 2; return n <= 0 ? -k : c(n - 1); }
  function c(n: number): number { k += 3; return n <= 0 ? k * 10 : a(n - 1); }
  return a(7);
}
console.log(three());

// the same shape with no capture
function outer(x: number): boolean {
  function isEven(n: number): boolean { return n === 0 ? true : isOdd(n - 1); }
  function isOdd(n: number): boolean { return n === 0 ? false : isEven(n - 1); }
  return isEven(x);
}
console.log(outer(4), outer(7));

// arrow body
const f = (start: number): number => {
  let hits = 0;
  function ping(n: number): number { hits++; return n <= 0 ? hits : pong(n - 1); }
  function pong(n: number): number { hits++; return n <= 0 ? -hits : ping(n - 1); }
  return ping(start);
};
console.log(f(4), f(5));

// closure bindings in an arrow body that call each other
const pair = (): number => {
  let c = 0;
  const ping = (n: number): number => { c++; return n <= 0 ? c : pong(n - 1); };
  const pong = (n: number): number => { c++; return n <= 0 ? -c : ping(n - 1); };
  return ping(3);
};
console.log(pair());

// three-way in an arrow, called before the declarations
const threeEarly = (n: number): number => {
  let k = 0;
  const r = a(n);
  return r * 100 + k;
  function a(m: number): number { k += 1; return m <= 0 ? 1 : b(m - 1); }
  function b(m: number): number { k += 2; return m <= 0 ? 2 : c(m - 1); }
  function c(m: number): number { k += 3; return m <= 0 ? 3 : a(m - 1); }
};
console.log(threeEarly(5), threeEarly(7));

// class method body, declared after the return
class C {
  scale = 2;
  go(n: number): number {
    let acc = this.scale;
    return up(n);
    function up(m: number): number { acc += 1; return m <= 0 ? acc : down(m - 1); }
    function down(m: number): number { acc *= 2; return m <= 0 ? -acc : up(m - 1); }
  }
}
console.log(new C().go(3), new C().go(4));

// declared after the return, with and without a capture
function late(n: number): number {
  let c = 10;
  return a(n);
  function a(m: number): number { c++; return m <= 0 ? c : b(m - 1); }
  function b(m: number): number { c++; return m <= 0 ? -c : a(m - 1); }
}
console.log(late(3), late(4));
function lateNoCap(n: number): boolean {
  return ev(n);
  function ev(m: number): boolean { return m === 0 ? true : od(m - 1); }
  function od(m: number): boolean { return m === 0 ? false : ev(m - 1); }
}
console.log(lateNoCap(6), lateNoCap(3));

// self-recursive, called before its text
function selfEarly(): number {
  let count = 0;
  const r = h(5);
  function h(n: number): number { count++; return n === 0 ? count : h(n - 1); }
  return r;
}
console.log(selfEarly());

// an early call that reads a binding still in its dead zone throws
function tdz(): string {
  try { return String(early()); } catch (e) { return (e as Error).name; }
  let later = 5;
  function early(): number { return later; }
}
console.log(tdz());

// an arrow above the declaration, called after a capture was reassigned
function viaArrow(): number {
  const h = () => sq(3);
  let base = 2;
  function sq(n: number): number { return n * n + base; }
  base = 10;
  return h();
}
console.log(viaArrow());

// a block function is initialized on block entry
function blk(n: number): number {
  let out = 0;
  {
    let c = 1;
    out = fb(n);
    function fb(m: number): number { c++; return m === 0 ? c : gb(m - 1); }
    function gb(m: number): number { c++; return m === 0 ? -c : fb(m - 1); }
  }
  return out;
}
console.log(blk(3));
