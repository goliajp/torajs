// writing null / undefined through `any` into a typed struct field: a
// string or heap-cell field holds them in its own slot, so typed reads
// see the new value, as the any lane does
const x = { toks: [1, 2], n: 3, s: "a", o: { k: 1 }, f: (v: number) => v + 1 };
const ax: any = x;
ax.s = undefined;
console.log(x.s, x.s === undefined);
ax.s = null;
console.log(x.s, x.s === null);
ax.s = "back";
console.log(x.s, x.s.length);
ax.toks = null;
console.log(x.toks, x.toks === null, typeof x.toks);
try {
  x.toks.join();
} catch (e: any) {
  console.log("caught", e instanceof TypeError);
}
ax.o = undefined;
console.log(x.o, x.o === undefined, typeof x.o);
ax.f = null;
console.log(ax.s, ax.toks, ax.o, ax.f);
console.log(JSON.stringify(x));
const x2 = { n: 3, f: (v: number) => v + 1, o: { k: 1 }, a: [1] };
const ax2: any = x2;
ax2.f = null; ax2.o = null; ax2.a = undefined;
console.log(JSON.stringify(x2));
console.log(x2);
const y: any = { m: x2 };
console.log(JSON.stringify(y));
