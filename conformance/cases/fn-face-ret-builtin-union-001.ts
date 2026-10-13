// A function declared to return a scalar hands back, as is, a value it
// reads off a union of builtin types (§10.2.1.12 converts nothing): a
// method answers what the library declares for each arm, an element of
// an `any[]` arm may be of any kind, and a parameter annotated such a
// union holds a value of one of its arms.
function f1(x: any[] | string): number { return x[0]; }
function f2(x: string | number): number { return x.toString(); }
function f3(x: any[] | number[]): number { return x[0]; }
function f4(x: string | boolean): number { return x.toString(); }
function f5(x: any[] | string): number { return x.slice(0); }
function f6(x: any[] | string[]): number { return x.filter(v => true)[0]; }
console.log(f1("abc"), f1(["q"]), f2("abc"), f2(7) + 1, f3(["n"]), f4(true));
console.log(f5("xy"), f6(["t"]));

type T = any[] | string[];
function g1(x: T): number { return x.map(v => v)[0]; }
function g2(x: T): number { return x[1]; }
function g3(x: T): number { return x.values().next().value; }
function g4(x: T): number { return x.at(0); }
console.log(g1(["b"]), g2(["d", "e"]), g3(["f"]), g4(["a"]));

// a member whose kind every arm fixes keeps the declared return
type S = string;
function h1(x: S[] | number[], i: number): number { return x[i]; }
function h2(x: number[] | string): number { return x.indexOf("b"); }
function h3(x: string | boolean): string { return x.toString(); }
function h4(x: string | any[]): string { return x.length as any; }
console.log(h1(["p", "q"], 1), h1([4, 5], 0) + 1, h2("abc"), h3(true), typeof h4("ab"));

// a parameter or a top-level binding annotated such a union
function k1(x: string | number): number { return x as any; }
function k2(x: bigint | number): number { return x.valueOf() as any; }
const c: any[] | string = "cq";
function k3(): number { return c[0]; }
function k4(x: any[] | string) { return x[0]; }
function k5(): number { return k4("zz"); }
console.log(k1("q"), k2(5n), k3(), k5());

// a guard narrows the arms a member is read off
function n1(v: string | number): number {
  if (typeof v === "string") return v.at(0) as any;
  return v;
}
function n2(v: string | number): string {
  if (typeof v === "number") return v.toFixed(1);
  return v.toUpperCase();
}
function n3(v: string | number): number {
  if (typeof v !== "string") return v;
  return v.length;
}
console.log(n1("ab"), n1(3) + 1, n2(2), n2("x"), n3(4) + 1, n3("four"));

// a field the program adds to an array, or reads by a string key, may
// hold a value of any kind
function e1(n: number): number {
  const xs: number[] = [n];
  (xs as any).tag = { length: "z" };
  return (xs as any).tag.length;
}
function e2(xs: number[]): number {
  const { ["1.5"]: v } = xs as any;
  return v;
}
const ea: number[] = [1, 2];
(ea as any)["1.5"] = "half";
console.log(e1(1), e2(ea));
