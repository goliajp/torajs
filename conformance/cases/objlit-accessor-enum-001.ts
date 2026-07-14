// RFC 20260714-objlit-accessor — an accessor slot lives in the layout
// under a synthetic name (`__getter_v` / `__setter_v`), but ES §10.4
// makes it ONE own property keyed by the plain name. Every enumeration
// surface must answer `v`, once — never the internal spelling.

const g = { a: 1, get v(): number { return 2; } };
console.log(Object.keys(g).join("|"));
console.log(Object.getOwnPropertyNames(g).join("|"));

const forin: string[] = [];
for (const k in g) { forin.push(k); }
console.log(forin.join("|"));

// get + set on the same property = one own key, not two.
let stored: number = 0;
const gs = {
  a: 1,
  get v(): number { return stored; },
  set v(x: number) { stored = x; },
};
console.log(Object.keys(gs).join("|"));
console.log(Object.getOwnPropertyNames(gs).join("|"));

// setter-only property still enumerates under its plain name.
const so = { b: 7, set w(x: number) { stored = x; } };
console.log(Object.keys(so).join("|"));

// the any lane (runtime layout walk) must agree with the typed lane.
const anyg: any = g;
console.log(Object.keys(anyg).join("|"));

// method shorthand is a plain own key (not an accessor slot).
const m = { a: 1, mm(): number { return 3; } };
console.log(Object.keys(m).join("|"));

// a plain object is untouched by the demangle.
console.log(Object.keys({ x: 1, y: 2 }).join("|"));
