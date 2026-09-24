// Array-literal elisions and map's hole arm now make the slot-value hole
// (`0` under FLAG_ARR_ANY_HOLEY) instead of a shadow entry; every reader
// must still see an absent index (§13.2.4.1, §10.4.2).
const mk = (): any => [0, , 2, , 4];
let a = mk();
console.log("in", 1 in a, 0 in a, a.hasOwnProperty(3), Object.keys(a).join(), JSON.stringify(Object.getOwnPropertyDescriptor(a, 1)));
console.log("idx", a.indexOf(undefined), a.includes(undefined), a.lastIndexOf(undefined), a.findIndex((x: any) => x === undefined));
console.log("str", a.join("-"), String(a), JSON.stringify(a), a.toString());
console.log("print", a);
console.log("spread", [...a].length, JSON.stringify([...a]), Array.from(a).length, Object.keys(Array.from(a)).join());
let fo = ""; for (const x of a) fo += x + ","; console.log("forof", fo);
let fi = ""; for (const k in a) fi += k + ","; console.log("forin", fi);
let fe = ""; a.forEach((x: any, i: any) => fe += i + ":" + x + ","); console.log("forEach", fe);
const m = a.map((x: any) => x * 2); console.log("map", m, 1 in m, m.length);
console.log("filter", a.filter(() => true), "every", a.every((x: any) => x !== undefined), "some", a.some((x: any) => x === undefined));
console.log("reduce", a.reduce((s: any, x: any) => s + x, 0), a.reduceRight((s: any, x: any) => s + "," + x));
const cc = a.concat([7, , 9]); console.log("concat", cc, 1 in cc, 6 in cc);
console.log("flat", [[1, , 3], , [4]].flat(), "flatMap", a.flatMap((x: any) => [x]));
let b = mk(); b.reverse(); console.log("reverse", b, 1 in b, 3 in b);
let c = mk(); c.sort(); console.log("sort", c, c.length, 3 in c, 4 in c);
let d = mk(); d.copyWithin(0, 1, 4); console.log("copyWithin", d, 0 in d, 1 in d, 2 in d);
let e = mk(); console.log("pop", e.pop(), e.pop(), e, e.length);
let f = mk(); console.log("shift", f.shift(), f.shift(), f, 0 in f, 1 in f);
let g = mk(); g.unshift(9); console.log("unshift", g, 2 in g);
let h = mk(); console.log("splice", h.splice(1, 2), h, 1 in h);
let i = mk(); i[1] = "x"; i[3] = undefined; console.log("revive", i, 1 in i, 3 in i);
let j = mk(); delete j[0]; console.log("delete", j, 0 in j);
let k = mk(); k.length = 7; console.log("grow", k, 5 in k); k.length = 2; console.log("shrink", k, 1 in k);
console.log("entries", JSON.stringify([...a.entries()]), JSON.stringify(Object.entries(a)));
const [p0, p1, p2] = a; console.log("destr", p0, p1, p2);
console.log("at", a.at(1), a[1], a[3], a.find((x: any) => x === undefined));
const n: any = [, ,]; console.log("allholes", n, n.length, 0 in n, JSON.stringify(n));
Array.prototype[1] = "P"; console.log("proto", a[1], 1 in a, a.join(), [...a].join()); delete Array.prototype[1];
const q = [1, , 3].map((x: any) => x); console.log("mapholes", q, 1 in q, Object.keys(q).join());
const s: any = new Set([0, , 2]); console.log("set", s.size, [...s]);
console.log("fill", JSON.stringify(mk().fill(1)), JSON.stringify(mk().with(0, 9)), JSON.stringify(mk().toSorted()), JSON.stringify(mk().toReversed()));
