// A binding annotated with an object type holds an `any` value as it
// is, wherever the value comes from: a function returning `any`, a
// later assignment of the binding (a local, a module global, one a
// closure captures), and a callback handing it back. The value need
// not have the type's fields: a member it lacks reads `undefined`, a
// member of another type reads as what it is, and the value keeps its
// identity. A scalar binding assigned an `any` value later holds it
// the same way.
type P = { x: number; y: number };
function attempt(tag: string, f: () => unknown): void {
  try {
    console.log(tag, f());
  } catch (e) {
    console.log(tag, "threw", (e as Error).constructor.name);
  }
}
function give(v: any): any {
  return v;
}
const vals: any[] = [{ x: 1, y: 2 }, { x: "s" }, 5, null, [3, 4]];
for (const v of vals) {
  attempt("call", () => {
    const p: P = give(v);
    return [typeof p, p === v, p?.x, p?.y].join(" ");
  });
  attempt("local", () => {
    let p: P = { x: 0, y: 0 };
    p = v;
    return [typeof p, p === v, p?.x].join(" ");
  });
  attempt("captured", () => {
    let p: P = { x: 0, y: 0 };
    const set = (): void => {
      p = v;
    };
    set();
    return [typeof p, p === v, p?.y].join(" ");
  });
}
let g: P = { x: 9, y: 9 };
function setG(v: any): void {
  g = v;
}
setG({ x: "gx" });
console.log("global", g.x, g.y, typeof g);
setG(7);
console.log("global", typeof g, g);
function show(tag: string, f: () => P): void {
  const p = f();
  console.log(tag, p.x, p.y);
}
const held: any = { x: 2.5, y: 3.5 };
show("callback", () => held);
show("callback call", () => give({ x: 1, y: 2 }));
let n: number = 1;
const s: any = "str";
n = s;
console.log("scalar", n, typeof n);
let flag: boolean = false;
flag = give(0);
console.log("scalar", flag, typeof flag);
