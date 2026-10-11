// Getters on heap-typed declared fields (string, object, array,
// boolean, any) answer through the typed read, each read calling the
// getter again; a setter on a string field receives the value and the
// assignment still answers its right-hand side.
class Box {
  s: string; o: Box | null; xs: number[]; b: boolean; a: any;
  constructor(s: string) { this.s = s; this.o = null; this.xs = [1]; this.b = false; this.a = 1; }
}
function show(x: Box): string {
  return x.s + "|" + (x.o === null ? "null" : x.o.s) + "|" + x.xs.length + "|" + x.b + "|" + x.a;
}
const inner = new Box("inner");
const k = new Box("k");
let n = 0;
Object.defineProperty(k, "s", { get() { n++; return "S" + n; }, configurable: true });
Object.defineProperty(k, "o", { get() { return inner; } });
Object.defineProperty(k, "xs", { get() { return [7, 8, 9]; } });
Object.defineProperty(k, "b", { get() { return true; } });
Object.defineProperty(k, "a", { get() { return "any!"; } });
console.log(show(k), n);
console.log(k.s + k.s, n);
console.log(show(new Box("plain")));
let kept = "";
Object.defineProperty(k, "s", { get() { return kept; }, set(v: string) { kept = v + v; } });
function put(x: Box, v: string): string { const r = (x.s = v + "!"); return r + "/" + x.s; }
console.log(put(k, "ab"), kept);
console.log(put(new Box("z"), "cd"));
