// §7.3.25 CopyDataProperties of a typed spread source whose accessor
// member a delete through an `any` alias removed: the source owns no
// such key, so the copy has none (an earlier member of the literal
// keeps its value); once a store recreated it as a data property, the
// copy takes that value. An accessor no delete removed still
// contributes its getter's answer, and a destructuring rest copies
// the same way.
const a = { get n(): number { return 7 }, k: 1 };
console.log(JSON.stringify({ ...a }));
const aa: any = a;
delete aa.n;
const c1 = { ...a };
console.log(JSON.stringify(c1), "n" in c1, Object.keys(c1).join(","), c1.n);
const c2 = { n: 3, ...a };
console.log(JSON.stringify(c2), Object.keys(c2).join(","));
const { k, ...rest } = a;
console.log(k, JSON.stringify(rest), "n" in rest);
aa.n = 8;
const c3 = { ...a };
console.log(c3.n, "n" in c3);
const b = { get n(): number { return 2 }, get w(): string { return "w" } };
const c4 = { ...b, z: 0 };
console.log(c4.n + 1, c4.w, Object.keys(c4).join(","));
const g = { get box() { return { v: 1 } }, q: "q" };
const ga: any = g;
delete ga.box;
const c5 = { ...g };
console.log(JSON.stringify(c5), c5.box);
const h = { get box() { return { v: 2 } } };
const c6 = { ...h };
console.log(JSON.stringify(c6));
