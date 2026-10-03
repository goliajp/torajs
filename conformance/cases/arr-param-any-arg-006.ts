// Holes and nulls behind a `Tok[]` parameter reached through `any`:
// `new Array<Tok>(n)`, a length grow, a delete and a `(Tok | null)[]`
// all leave an index with no Tok, so `.value` on it throws a
// TypeError in the body; the elements that are there read as usual.
class Tok {
  kind: number;
  value: number;
  constructor(kind: number, value: number) {
    this.kind = kind;
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
  last(toks: Tok[]) {
    return toks[toks.length - 1].value;
  }
  count(toks: Tok[]) {
    return toks.length;
  }
}
const r: any = new Reader(100);
console.log("typed ->", new Reader(100).first([new Tok(0, 8)]), new Reader(100).last([new Tok(0, 9)]));
const sized: Tok[] = new Array<Tok>(3);
const withNull: (Tok | null)[] = [null, new Tok(0, 2)];
const some: (Tok | null)[] = [new Tok(0, 5), null];
const grown: Tok[] = [new Tok(0, 1)];
grown.length = 3;
const holed: Tok[] = [new Tok(0, 1), new Tok(0, 2)];
delete holed[1];
console.log("count ->", r.count(sized), r.count(withNull), r.count(some), r.count(grown), r.count(holed));
console.log("present ->", r.first(some), r.first(grown), r.first(holed));
for (const bad of [sized, withNull]) {
  try {
    console.log("bad ->", r.first(bad));
  } catch (e) {
    console.log("first throws", e instanceof TypeError);
  }
}
for (const bad of [some, grown, holed]) {
  try {
    console.log("bad ->", r.last(bad));
  } catch (e) {
    console.log("last throws", e instanceof TypeError);
  }
}
