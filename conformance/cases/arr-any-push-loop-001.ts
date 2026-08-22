// The push-loop pre-reserve fast path caches the array's length across
// the loop and writes it back at the exit. `Array<Any>.push` needs a
// (tag, value) pair and never takes that path, so a claimed `any[]`
// binding got the reserve and the cached length, none of the inline
// stores, and then had its PRE-LOOP length written back over
// everything the runtime helper had appended.
let a: any[] = [];
for (let i: number = 0; i < 5; i = i + 1) { a.push(i); }
console.log("for  :", a.length, JSON.stringify(a));

let b: any[] = [];
let j: number = 0;
while (j < 5) { b.push(j); j = j + 1; }
console.log("while:", b.length, JSON.stringify(b));

// A non-empty init made the erasure look like a partial one.
let c: any[] = [0];
for (let i: number = 0; i < 3; i = i + 1) { c.push("s"); }
console.log("seed :", c.length, JSON.stringify(c));

// Any other statement in the body kept the binding live and hid it.
let d: any[] = [];
for (let i: number = 0; i < 3; i = i + 1) { d.push(i); console.log("tick", d.length); }
console.log("live :", d.length);

// Two arrays filled in lockstep: the fast path serves the typed one and
// declines the boxed one.
let t: number[] = [];
let u: any[] = [];
for (let i: number = 0; i < 4; i = i + 1) { t.push(i); u.push(i); }
console.log("pair :", t.length, u.length, JSON.stringify(t), JSON.stringify(u));

// The reads that go through it agree afterwards.
let sum: number = 0;
u.forEach(function (v: any) { sum = sum + (v as number); });
console.log("read :", sum, u.reduce(function (p: any, v: any) { return (p as number) + (v as number); }, 0));
