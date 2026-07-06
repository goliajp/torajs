// chunk 568 — the consuming-params bitmap is retired: fn args always SHARE
// (chunks 564-567 gave every store lane its own +1, so the caller-side
// consume double-counted into an orphaned stake).

// 1. method this.f = param: source stays readable and re-usable
class K {
  f: string = "";
  keep(s: string) { this.f = s; }
}
let k = new K();
let a = "AAAA" + 1;
k.keep(a);
k.keep("BBBB" + 2);
console.log(a);
console.log(k.f);

// 2. ctor field store: source outlives field re-assign
class P {
  f: string;
  constructor(s: string) { this.f = s; }
}
let b = "CCCC" + 3;
let p = new P(b);
p.f = "DDDD" + 4;
let canary = "EEEE" + 5;
console.log(b);
console.log(p.f);
console.log(canary);

// 3. same binding into two consuming-shaped calls
let c = "FFFF" + 6;
let p1 = new P(c);
let p2 = new P(c);
console.log(c);
console.log(p1.f);
console.log(p2.f);

// 4. alias binding into a consuming-shaped fn (was a loud rejection path)
let box4 = { t: "GGGG" + 7 };
let al = box4.t;
let p3 = new P(al);
console.log(al);
console.log(p3.f);
console.log(box4.t);
