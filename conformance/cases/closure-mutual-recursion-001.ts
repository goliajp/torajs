// Two closure bindings that call each other. The first one's capture
// list names the second, which is not declared until the next
// statement, so resolving captures against the scope as it stands
// answered "unknown identifier". ES hoists `let` / `const` to the top
// of their block, and neither body runs until both declarations have,
// so nothing is actually being read early.
//
// The companion of closure-self-reference-001: same capture box, same
// reference cycle for the collector to break, but the box has to be
// opened a statement before the one that fills it.

function pingPong(): void {
  const ping = (n: number): number => (n <= 0 ? 0 : 1 + pong(n - 1));
  const pong = (n: number): number => (n <= 0 ? 0 : 1 + ping(n - 1));
  console.log(ping(6), pong(5), ping(0), pong(1));
}

// a three-way cycle, and a self-referential binding living among them
function threeWay(): void {
  const x = (n: number): number => (n <= 0 ? 0 : y(n - 1));
  const y = (n: number): number => (n <= 0 ? 1 : z(n - 1));
  const z = (n: number): number => (n <= 0 ? 2 : x(n - 1));
  const own = (n: number): number => (n <= 0 ? 9 : own(n - 1));
  console.log(x(4), y(4), z(4), own(3));
}

// block scope gets its own pass, and the pair may capture other things
function inBlock(): void {
  const base = 100;
  {
    const a = (n: number): number => (n <= 0 ? base : b(n - 1));
    const b = (n: number): number => (n <= 0 ? base * 2 : a(n - 1));
    console.log(a(3), b(3));
  }
}

// function expressions, and a mutual pair that escapes its frame
function escaping(): (n: number) => boolean {
  const even = function (n: number): boolean {
    return n === 0 ? true : odd(n - 1);
  };
  const odd = function (n: number): boolean {
    return n === 0 ? false : even(n - 1);
  };
  return even;
}

// a later binding capturing an EARLIER one needs nothing special —
// by then the binding is ordinary and already there
function backwardOnly(): void {
  const first = (n: number): number => n * 2;
  const second = (n: number): number => first(n) + 1;
  console.log(second(5));
}

function main(): void {
  pingPong();
  threeWay();
  inBlock();
  const isEvenEscaped = escaping();
  console.log(isEvenEscaped(10), isEvenEscaped(7));
  backwardOnly();
}

main();

// and the same pair at module top level
const topEven = (n: number): boolean => (n === 0 ? true : topOdd(n - 1));
const topOdd = (n: number): boolean => (n === 0 ? false : topEven(n - 1));
console.log(topEven(10), topOdd(7), topEven(3), topOdd(4));
