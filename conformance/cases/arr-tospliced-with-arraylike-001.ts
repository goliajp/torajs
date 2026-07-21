// arraylike receiver toSpliced / with — §23.1.3.35 / §23.1.3.39
// generic semantics: exact read sets (the skipped range must NOT
// fire its getters), with's RangeError precedes any element Get.
let getCalls: any = [];
const mk = () => ({
  length: 4,
  get 0() { getCalls.push(0); return "a"; },
  get 1() { getCalls.push(1); return "b"; },
  get 2() { getCalls.push(2); return "c"; },
  get 3() { getCalls.push(3); return "d"; },
});

// toSpliced skips the removed range's getters: reads [0) + [2,4)
getCalls = [];
const sp = (Array.prototype as any).toSpliced.call(mk(), 1, 1, "X", "Y");
console.log(JSON.stringify(sp), JSON.stringify(getCalls));

// argc==1: skip to end — only [0,1) read
getCalls = [];
const sp1 = (Array.prototype as any).toSpliced.call(mk(), 1);
console.log(JSON.stringify(sp1), JSON.stringify(getCalls));

// argc==0: full copy
getCalls = [];
const sp0 = (Array.prototype as any).toSpliced.call(mk());
console.log(JSON.stringify(sp0), JSON.stringify(getCalls));

// negative start wraps
getCalls = [];
const spn = (Array.prototype as any).toSpliced.call(mk(), -2, 5, "Z");
console.log(JSON.stringify(spn), JSON.stringify(getCalls));

// with: substitution index's getter must NOT fire
getCalls = [];
const w = (Array.prototype as any).with.call(mk(), 2, "W");
console.log(JSON.stringify(w), JSON.stringify(getCalls));

// with negative index
getCalls = [];
const wn = (Array.prototype as any).with.call(mk(), -1, "N");
console.log(JSON.stringify(wn), JSON.stringify(getCalls));

// with OOB throws RangeError BEFORE any element Get
getCalls = [];
try {
  (Array.prototype as any).with.call(mk(), 4, "x");
} catch (e: any) {
  console.log("oob:", e instanceof RangeError, JSON.stringify(getCalls));
}

// own-prop install shape re-enters the generic arm
const o2: any = { 0: "p", 1: "q", length: 2 };
o2.with = (Array.prototype as any).with;
console.log(JSON.stringify(o2.with(0, "R")));
