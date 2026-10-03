// §10.2.1.3 — the array a `Tok[]` parameter receives through `any`
// is the caller's own, not a copy: a typed array of Tok is pushed to
// and overwritten in place, and so is an array of object literals,
// each element read and written through [[Get]] / [[Set]].
class Tok {
  kind: number;
  value: number;
  constructor(kind: number, value: number) {
    this.kind = kind;
    this.value = value;
  }
}
class Builder {
  append(toks: Tok[], kind: number, value: number) {
    toks.push(new Tok(kind, value));
    toks[0].value = toks[0].value + 100;
    return toks.length;
  }
}
const b: any = new Builder();
const typed: Tok[] = [new Tok(0, 1)];
console.log("tok ->", b.append(typed, 1, 2), typed.length, typed[0].value, typed[1].value);
const lits: any = [{ value: 5 }];
console.log("literal ->", b.append(lits, 1, 2), lits.length, lits[0].value, lits[1].value, lits[1] instanceof Tok);
const empty: any = [];
console.log("empty ->", b.append(empty, 2, 3), empty.length, empty[0].value);
