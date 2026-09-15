// RFC 20260915 — the obj / index of a compound or logical assignment
// target evaluate exactly once (§13.15.2 evaluates the Reference once).
let a: number[] = [10, 20, 30];
let i = 0;
a[i++] += 5;
console.log(a.join(","), i);
a[i++] -= 1;
console.log(a.join(","), i);
a[i++] |= 1;
console.log(a.join(","), i);

let calls = 0;
const arr: any = [1, 2, 3];
function f(): any {
  calls++;
  return arr;
}
f()[1] *= 2;
console.log(arr.join(","), calls);
f()[2] **= 2;
console.log(arr.join(","), calls);

let keys = 0;
const o: any = { x: 1, s: "a" };
function key(k: string): string {
  keys++;
  return k;
}
o[key("x")] += 41;
o[key("s")] += "b";
console.log(o.x, o.s, keys);

o[key("y")] ??= 7;
o[key("y")] ??= 8;
o[key("x")] ||= 100;
o[key("z")] ||= "z";
o[key("s")] &&= o.s + "c";
console.log(o.x, o.y, o.z, o.s, keys);

let mcalls = 0;
class P {
  n = 1;
  t = "p";
}
const p = new P();
function g(): P {
  mcalls++;
  return p;
}
g().n += 2;
g().t += "q";
console.log(p.n, p.t, mcalls);
const anyp: any = p;
function h(): any {
  mcalls++;
  return anyp;
}
h().n *= 10;
h().n ??= -1;
const kept = h().t ||= "never";
console.log(p.n, kept, mcalls);

const log: string[] = [];
const t: any = { k: 1 };
function ob(): any {
  log.push("obj");
  return t;
}
function kk(): string {
  log.push("key");
  return "k";
}
function rv(): number {
  log.push("rhs");
  return 2;
}
ob()[kk()] += rv();
console.log(t.k, log.join(" "));
log.length = 0;
ob()[kk()] ??= rv();
console.log(t.k, log.join(" "));

const nums: number[] = [1, 2, 3];
let j = 0;
function idx(): number {
  j++;
  return 1;
}
nums[idx()] += nums[idx()];
console.log(nums.join(","), j);
