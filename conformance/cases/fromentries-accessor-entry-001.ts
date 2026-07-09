// chunk 746 — Object.fromEntries accessor entries: ES §20.1.2.7
// AddEntriesFromIterable reads Get(entry, "0") / Get(entry, "1"),
// which invokes a getter when the slot is an accessor (§7.3.24
// [[Get]]). The OWNED getter answer transfers to the built slot
// (value side) or drops after ToPropertyKey (key side). Covers the
// array lane, the Set lane, and heap-string getter answers.
const e1: any = {};
Object.defineProperty(e1, "0", { get() { return "k"; }, enumerable: true });
Object.defineProperty(e1, "1", { get() { return 42; }, enumerable: true });
const o1: any = Object.fromEntries([e1]);
console.log(o1.k);
const e2: any = { 0: "plain", 1: 1 };
Object.defineProperty(e2, "1", { get() { return 99; }, enumerable: true });
const s = new Set([e2]);
const o2: any = Object.fromEntries(s);
console.log(o2.plain);
const e3: any = {};
Object.defineProperty(e3, "0", { get() { return "key" + "X"; }, enumerable: true });
Object.defineProperty(e3, "1", { get() { return "val" + "Y"; }, enumerable: true });
const o3: any = Object.fromEntries([e3]);
console.log(o3.keyX);
