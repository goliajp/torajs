// The four reflection surfaces over a bag-only shape must agree with
// each other: a property that `Object.keys` counts is one
// `Object.values` and `Object.entries` count too. They used to read
// three separate copies of "which shapes keep a bag", so a shape could
// be admitted to one and missing from another — `Object.keys(wm)`
// answered 1 while `Object.values(wm)` answered 0.

function surfaces(o: any): string {
  return Object.keys(o).length + "/" + Object.values(o).length + "/" +
         Object.entries(o).length + "/" + Object.getOwnPropertyNames(o).length;
}

const m = new Map([["e", 1]]); (m as any).a = 1; (m as any)[0] = 2;
const mx: any = m;
console.log(surfaces(mx));
console.log(Object.keys(mx).join(","));
console.log(Object.values(mx).join(","));
console.log(m.size);

const wm = new WeakMap(); (wm as any).a = 1;
const wmx: any = wm;
console.log(surfaces(wmx));

const ws = new WeakSet(); (ws as any).b = 2;
const wsx: any = ws;
console.log(surfaces(wsx));

const wr = new WeakRef({ id: 1 }); (wr as any).c = 3;
const wrx: any = wr;
console.log(surfaces(wrx));

const d = new Date(0); (d as any).e = 4;
const dx: any = d;
console.log(surfaces(dx));

const r = /x/g; (r as any).f = 5;
const rx: any = r;
console.log(surfaces(rx));
console.log(r.lastIndex);
