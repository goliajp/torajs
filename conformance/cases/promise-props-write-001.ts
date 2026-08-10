// Promise expando plain-assign (member_set Tag::Promise arm) — the
// write twin of promise-props-read-001: `p.foo = v` lands in the +32
// bag the get channel probes; an own `then` shadows the prototype
// surface on read.
const p = Promise.resolve(1);
(p as any).foo = 42;
console.log("r1", (p as any).foo);
console.log("r2", (p as any).bar);
(p as any).foo = 43;
console.log("r3", (p as any).foo);
const p2 = Promise.resolve(2);
(p2 as any).then = "shadow";
console.log("r4", (p2 as any).then);
p.then((v) => console.log("r5", v));
