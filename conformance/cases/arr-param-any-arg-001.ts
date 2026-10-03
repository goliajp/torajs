// §13.3.2.1 / §10.2.1.3 — an array handed to a `Tok[]` parameter
// through `any`: a typed array of Tok (or of a class extending it)
// takes the typed body; an array of another class, of object
// literals, of strings or a mix reads each element's own `value`
// through [[Get]]; `null` / `undefined` throw on `.length`.
class Tok {
  kind: number;
  value: number;
  constructor(kind: number, value: number) {
    this.kind = kind;
    this.value = value;
  }
}
class Sub extends Tok {
  extra: number;
  constructor(value: number) {
    super(1, value);
    this.extra = 3;
  }
}
class Other {
  name: string;
  value: string;
  constructor(value: string) {
    this.name = "o";
    this.value = value;
  }
}
class Reader {
  base: number;
  constructor(base: number) {
    this.base = base;
  }
  first(toks: Tok[]) {
    return toks[0].value;
  }
  sum(toks: Tok[]): number {
    let s = this.base;
    for (let i = 0; i < toks.length; i++) s = s + toks[i].value;
    return s;
  }
  grid(rows: Tok[][]) {
    return rows[rows.length - 1][0].value;
  }
}
const r: any = new Reader(100);
console.log("typed ->", new Reader(100).first([new Tok(0, 8)]), new Reader(100).sum([new Tok(0, 1), new Tok(0, 2)]));
console.log("tok ->", r.first([new Tok(0, 8)]), r.sum([new Tok(0, 1), new Tok(0, 2)]));
console.log("sub ->", r.first([new Sub(5)]), r.sum([new Sub(5), new Tok(0, 1)]));
console.log("other ->", r.first([new Other("o")]));
console.log("literal ->", r.first([{ value: 9 }]), r.sum([{ value: 9 }, { value: 1 }]));
console.log("string ->", r.first(["str"]));
console.log("mixed ->", r.sum([new Tok(0, 4), { value: 5 }]));
console.log("empty ->", r.sum([]));
console.log("grid tok ->", r.grid([[new Tok(0, 1)], [new Tok(0, 2)]]));
console.log("grid literal ->", r.grid([[new Tok(0, 1)], [{ value: 7 }]]));
for (const bad of [null, undefined, 3, "abc"]) {
  try {
    console.log("bad ->", r.sum(bad));
  } catch (e) {
    console.log("throws", e instanceof TypeError);
  }
}
