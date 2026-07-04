// L3b #3 (chunk 527) — numeric-key indexing on a dynobj through
// any: the key stringifies to its decimal form (o[42] === o["42"]
// per ES ToPropertyKey) for both reads and writes; a write that
// resizes the table writes the relocated cell back through the
// receiver's slot; a numeric-key accessor's getter answers as the
// value; absent keys answer undefined.
const o: any = { "0": "zero", "42": "answer", name: "n" };
console.log(o[0]);
console.log(o[42]);
console.log(o[1]);
const k = 42;
console.log(o[k]);
o[7] = "seven";
console.log(o[7]);
console.log(o.name);
const neg: any = { "-3": "minus" };
console.log(neg[-3]);
const grow: any = {};
for (let i = 0; i < 12; i++) {
  grow[i] = i * 10;
}
console.log(grow[0]);
console.log(grow[11]);
const acc: any = {};
Object.defineProperty(acc, "5", { get: () => "got5" });
console.log(acc[5]);
