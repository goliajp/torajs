// `Array.prototype.toSpliced(start, deleteCount?, ...)` per ES §23.1.3.42:
// the immutable sibling of `splice`. Both args are spec-optional;
// 0-arg gives a fresh shallow copy unchanged, 1-arg deletes from
// `start` to the end of the clone. Pre-fix tr required exactly 2 args
// (mirroring the splice gap fixed in the prior commit) and rejected
// the 0/1-arg forms.

const a = [1, 2, 3, 4, 5];
console.log("toSpliced(2):", JSON.stringify(a.toSpliced(2)));
console.log("a after:", JSON.stringify(a));

const b = [10, 20, 30, 40, 50];
console.log("toSpliced(-2):", JSON.stringify(b.toSpliced(-2)));
console.log("b after:", JSON.stringify(b));

const c = [1, 2, 3];
console.log("toSpliced():", JSON.stringify(c.toSpliced()));
console.log("c after:", JSON.stringify(c));

const d = [1, 2, 3];
console.log("toSpliced(0):", JSON.stringify(d.toSpliced(0)));
console.log("d after:", JSON.stringify(d));

const e = [1, 2, 3];
console.log("toSpliced(10):", JSON.stringify(e.toSpliced(10)));
console.log("e after:", JSON.stringify(e));

// 2-arg form unchanged.
const f = [1, 2, 3, 4, 5];
console.log("toSpliced(1,2):", JSON.stringify(f.toSpliced(1, 2)));
console.log("f after:", JSON.stringify(f));

const g: string[] = ["a", "b", "c"];
console.log("toSpliced(str,1):", JSON.stringify(g.toSpliced(1)));
console.log("g after:", JSON.stringify(g));
