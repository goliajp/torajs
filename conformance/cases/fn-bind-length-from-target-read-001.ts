// §20.2.3.2 steps 5-6 — a bound function's length comes from
// HasOwnProperty(Target, "length") then Get then ToIntegerOrInfinity,
// not from the target's registered arity. Each line below is a value
// the arity cannot express.
function mk(v: any): any {
  const f: any = function () {};
  Object.defineProperty(f, "length", { value: v });
  return f;
}
const values: any[] = [4.5, -1, "3", {}, Infinity, -Infinity, NaN, 4294967296];
for (let i = 0; i < values.length; i++) {
  const t: any = mk(values[i]);
  console.log(String(values[i]), (t.bind() as any).length, (t.bind(null, 1, 2) as any).length);
}
// a target with no own length at all: step 5 answers false, L is 0
const gone: any = function () {};
delete (gone as any).length;
console.log("deleted", (gone.bind() as any).length);
// redefining the same target repeatedly is observable each time
const fn: any = function () {};
Object.defineProperty(fn, "length", { value: -0 });
console.log("-0", (fn.bind() as any).length);
Object.defineProperty(fn, "length", { value: Infinity });
console.log("inf", (fn.bind() as any).length, (fn.bind(0, 0) as any).length);
Object.defineProperty(fn, "length", { value: 3.66 });
console.log("3.66", (fn.bind() as any).length);
// an ordinary target is unchanged
function plain(a: number, b: number, c: number): number {
  return a + b + c;
}
const p: any = plain;
console.log("plain", (p.bind(null) as any).length, (p.bind(null, 1) as any).length);
