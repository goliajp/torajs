// §13.3.6.1 — a comma expression in callee position is not a
// Reference: its left operands run for effect, then the last
// operand's value is called with an undefined `this`. Typed callees of
// that shape reached the direct-call resolver and were refused.
function f(n: number): number { return n + 1; }
const g = (n: number) => n * 2;
console.log((0, f)(1), (0, g)(2));
class C { h: (n: number) => number = (n) => n + 3; }
const c = new C();
console.log((0, c.h)(4));
let k = 0;
console.log((k++, f)(k), k);
const log: string[] = [];
const pick = (s: string) => { log.push(s); return g; };
console.log((log.push("left"), pick("right"))(5), log.join(","));
// a method read off a typed receiver loses the receiver as well
class D { tag = "d"; m(): string { return typeof this; } }
const d = new D();
console.log((d.tag, d.m)());
