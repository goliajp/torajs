// ES2021 §13.15: ||=, &&=, ??= must short-circuit — the assign is
// skipped entirely when the lhs is truthy / falsy / non-nullish
// respectively. Previously tr desugared these to `x = x op y`, which
// always calls PutValue and blows up on non-writeable targets.

// A: ||= truthy lhs — skip assign, non-writeable p not touched
const A: any = {};
Object.defineProperty(A, "p", { value: "kept", writable: false, configurable: true });
A.p ||= "assigned";
console.log("A:", A.p);

// B: &&= falsy lhs — skip assign
const B: any = {};
Object.defineProperty(B, "p", { value: 0, writable: false, configurable: true });
B.p &&= 999;
console.log("B:", B.p);

// C: ??= non-nullish lhs — skip assign
const C: any = {};
Object.defineProperty(C, "p", { value: 5, writable: false, configurable: true });
C.p ??= 999;
console.log("C:", C.p);

// D: ||= falsy lhs — assign fires
const D: any = { p: 0 };
D.p ||= 7;
console.log("D:", D.p);

// E: &&= truthy lhs — assign fires
const E: any = { p: 1 };
E.p &&= 42;
console.log("E:", E.p);

// F: ??= nullish lhs (null) — assign fires
const F: any = { p: null };
F.p ??= "filled";
console.log("F:", F.p);

// (BigInt short-circuit cases from test262 excluded here — tr has a
// pre-existing bug where every BigInt is truthy, so `0n &&= 1n`
// depends on a separate BigInt-truthiness fix; tracked in L3b.)
