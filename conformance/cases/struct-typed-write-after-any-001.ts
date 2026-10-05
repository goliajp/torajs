// a typed store into a field the any lane wrote past the slot's type:
// the typed value is the field's value from then on, on both lanes
const x = { n: 3, s: "a" };
const ax: any = x;
ax.n = "zz";
x.n = 5;
console.log(ax.n, x.n);
ax.s = 7;
x.s = "b";
console.log(ax.s, x.s, JSON.stringify(x));
const fr = { k: 1 };
Object.freeze(fr);
try {
  (fr as any).k = 2;
} catch (e: any) {
  console.log("frozen", e instanceof TypeError);
}
console.log(fr.k);
