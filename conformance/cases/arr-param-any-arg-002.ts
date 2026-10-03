// A top-level function and a static method with a `Tok[]` parameter
// reached through `any`: the same reading as a method's — a typed
// array of Tok runs the typed body, anything else reads each element
// through [[Get]], and a non-array throws on `.length`.
class Tok {
  kind: number;
  value: number;
  constructor(kind: number, value: number) {
    this.kind = kind;
    this.value = value;
  }
}
function total(toks: Tok[]): number {
  let s = 0;
  for (let i = 0; i < toks.length; i++) s = s + toks[i].value;
  return s;
}
class Counter {
  static last(toks: Tok[]) {
    return toks[toks.length - 1].value;
  }
}
const g: any = total;
const c: any = Counter;
console.log("typed ->", total([new Tok(0, 1), new Tok(0, 2)]), Counter.last([new Tok(0, 4)]));
console.log("tok ->", g([new Tok(0, 1), new Tok(0, 2)]), c.last([new Tok(0, 4)]));
console.log("literal ->", g([{ value: 9 }, { value: 1 }]), c.last([{ value: 6 }]));
console.log("string ->", g(["a", "b"]), c.last(["z"]));
for (const bad of [null, undefined]) {
  try {
    console.log("bad ->", g(bad));
  } catch (e) {
    console.log("throws", e instanceof TypeError);
  }
  try {
    console.log("bad ->", c.last(bad));
  } catch (e) {
    console.log("throws", e instanceof TypeError);
  }
}
