function mk(tag: string): any {
  return {
    [Symbol.iterator]() {
      let i = 0;
      return {
        next() { i++; return { value: i, done: i > 3 }; },
        return() { console.log("close", tag); return {}; },
      };
    },
  };
}
function boom(): number { throw new Error("boom"); }
function a(): void {
  for (const x of mk("a")) { console.log("a", x); throw new Error("a"); }
}
function b(): void {
  for (const x of mk("b-outer")) {
    for (const y of mk("b-inner")) { console.log("b", x, y); boom(); }
  }
}
function c(): void {
  for (const x of mk("c")) {
    try { console.log("c", x); } finally { console.log("c finally"); }
    boom();
  }
}
function d(): number {
  for (const x of mk("d")) {
    try { boom(); } finally { console.log("d finally"); }
  }
  return 0;
}
function* g(): Generator<number> { try { yield 1; yield 2; } finally { console.log("gen finally"); } }
function e(): void {
  for (const x of g()) { console.log("e", x); boom(); }
}
for (const fn of [a, b, c, d, e]) {
  try { fn(); } catch (err) { console.log("caught", (err as Error).message); }
}
