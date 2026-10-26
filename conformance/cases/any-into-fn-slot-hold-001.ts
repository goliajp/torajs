// A binding annotated with a function type holds an `any` value as it
// is: a function of any kind is called through its own entry, and a
// value that is no function is stored, keeps its `typeof` and its
// identity, and is a TypeError only when it is called (§7.3.13 Call).
function attempt(tag: string, f: () => unknown): void {
  try {
    console.log(tag, f());
  } catch (e) {
    console.log(tag, "threw", (e as Error).constructor.name);
  }
}
function base(this: any, x: number): number {
  return x + 1;
}
const vals: any[] = [() => "arrow", base.bind(null), Math.max, 5, "s", { call: 1 }, undefined];
for (const v of vals) {
  attempt("store", () => {
    const g: () => unknown = v;
    return [typeof g, g === v].join(" ");
  });
  attempt("call", () => {
    const g: (a: number, b: number) => unknown = v;
    return g(3, 4);
  });
  attempt("reassign", () => {
    let g: () => unknown = () => "first";
    g = v;
    return [typeof g, g === v].join(" ");
  });
  attempt("call method", () => {
    const g: (a: number) => unknown = v;
    return g.call(null, 2);
  });
}
let top: () => unknown = () => "top";
function setTop(v: any): void {
  top = v;
}
setTop(7);
console.log("global", typeof top, top);
setTop(() => "again");
console.log("global", top());
