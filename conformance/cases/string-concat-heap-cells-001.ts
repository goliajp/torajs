// §13.15.3 with a String side is a concat, and ToString answers for
// every heap cell: the any-lane kernel reads the header tag and gives
// §20.1.4.4's "[object X]", the Date string, or an iterator's name.
// `("x" as any) + new Map()` printed all of them all along — the
// static lane's list held RegExp and nothing else, so the same
// program with the `any` erased was a compile error.
const m = new Map<number, number>([[1, 2]]);
const s = new Set<number>([1]);
const wm = new WeakMap<object, number>();
const ws = new WeakSet<object>();
const wr = new WeakRef({});
const d = new Date(0);
const re = /a+/g;
const p = Promise.resolve(1);

console.log("x" + m, m + "x");
console.log("x" + s, s + "x");
console.log("x" + wm, "x" + ws, "x" + wr);
console.log("x" + p);
console.log("x" + re, re + "x");
console.log("x" + m.keys(), "x" + m.values(), "x" + m.entries());
console.log("x" + [1, 2].keys());
console.log(("x" + d) === ("x" + String(d)));

// the `String()` call face carries the same list, and it was written
// half-way in the same rotation that started it: Map / Set / RegExp
// went in, the Date / weak / iterator / Promise rows did not.
console.log(String(m), String(s), String(wm), String(ws), String(wr), String(p), String(re));
console.log(String(d) === d.toString(), String(m.keys()), String([1, 2].keys()));

// the `any` spelling, which has answered correctly all along
const a: any = "x";
console.log(a + m, a + s, a + wm, a + ws, a + wr, a + p, a + re);

// template literals take the same route
console.log(`${m}|${s}|${re}`);

// and the families that were already listed, unchanged
console.log("x" + [1, 2], "x" + { a: 1 }, "x" + null, "x" + undefined, "x" + true, "x" + 1n);
