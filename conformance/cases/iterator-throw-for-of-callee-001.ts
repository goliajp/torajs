// The for..of twin of iterator-throw-in-ctor-body-001: a loop over a
// source the checker cannot type as a static iterable runs the
// §7.4.2 protocol at runtime, and the throw-shape analysis now
// counts it — before, a named function whose only raise was its
// loop's `[Symbol.iterator]` had its caller's check elided, so
// `outer() → inner()` completed as if nothing had thrown.
class Bad { [Symbol.iterator]() { throw new Error("iter"); } }
class Late { [Symbol.iterator]() { let i = 0; return { next() { if (i++ < 2) return { value: i, done: false }; throw new Error("late"); } }; } }
function inner() { const b: any = new Bad(); let n = 0; for (const x of b) { n += x; } return n; }
function outer() { return inner(); }
try { outer(); console.log("no throw"); } catch (e: any) { console.log("caught", e.message); }
function innerLate() { const l: any = new Late(); const out: number[] = []; for (const x of l) { out.push(x); } return out; }
function outerLate() { return innerLate(); }
try { outerLate(); console.log("no throw late"); } catch (e: any) { console.log("caught late", e.message); }
class P { constructor() { } }
class C extends P { constructor() { super(); const b: any = new Bad(); for (const x of b) { console.log(x); } console.log("after loop"); } }
try { new C(); console.log("no throw C"); } catch (e: any) { console.log("caught C", e.message); }
function typedLoop(xs: number[]) { let s = 0; for (const x of xs) { s += x; } return s; }
function okAny() { const l: any = [1, 2, 3]; let s = 0; for (const x of l) { s += x; } return s; }
console.log(typedLoop([1, 2, 3]), okAny());
