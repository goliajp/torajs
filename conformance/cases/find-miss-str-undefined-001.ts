// RFC 20260722-find-miss chunk A — string[].find/findLast miss
// answers a real undefined (was NULL -> printed null).
const ss = ["alpha", "beta"];
const r = ss.find((v) => v === "zeta");
console.log(r);
console.log(typeof r);
console.log(r === undefined);
console.log(r == null);
const h = ss.find((v) => v === "beta");
console.log(h, typeof h, h === undefined);
const rl = ss.findLast((v) => v.length > 9);
console.log(rl, typeof rl);
const hl = ss.findLast((v) => v.length === 5);
console.log(hl);
// alias + template + truthiness consumers
const alias = ss.find((v) => v === "nope");
console.log(`${alias}`, alias ? "t" : "f");
