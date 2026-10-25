// The module-body twin of for-head-closure-copies-001: closures a
// `for (let …)` head makes at the top level (§14.7.4.4). An update's
// closure sees the increment after it, a test's its own iteration, an
// initializer's the head's own binding, also through a parameter
// default (§10.2.11).
const ups: (() => number)[] = [];
for (let i = 0; i < 3; ups.push(() => i), i++);
console.log("update", ups.map((h) => h()).join(","));

const upDefaults: ((y?: number) => number)[] = [];
for (let i = 0; i < 3; upDefaults.push((x: number = i) => x), i++);
console.log("updateDefault", upDefaults.map((h) => h()).join(","));

const strs: (() => string)[] = [];
for (let s = "a"; s.length < 4; strs.push(() => s), s = s + "b");
console.log("updateString", strs.map((h) => h()).join(","));

const inits: number[] = [];
let last: () => number = () => -1;
for (let i = 0, g = () => i; i < 3; i++) {
  inits.push(i, g());
  last = g;
}
inits.push(last());
console.log("init", inits.join(","));

const writes: number[] = [];
for (let i = 0, bump = () => ++i; i < 3; i++) writes.push(i, bump());
console.log("initWrites", writes.join(","));

const defaults: number[] = [];
for (let i = 0, g = (x: number = i) => x; i < 3; i++) defaults.push(g());
console.log("initDefault", defaults.join(","));

const bodies: (() => number)[] = [];
for (let i = 0; i < 4; bodies.push(() => i * 10), i++) {
  bodies.push(() => i);
  if (i === 1) continue;
  if (i === 2) break;
}
console.log("withBody", bodies.map((h) => h()).join(","));
