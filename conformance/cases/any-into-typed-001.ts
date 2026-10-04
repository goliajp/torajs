// A method reached through `any` binds each parameter to the argument
// as passed: `null` / `undefined` / a value of another kind behind a
// `Tok[]`, `number[]` or `string` parameter is that value in the body,
// and reading a property of `null` throws a TypeError.
class Tok {
  k: number;
  v: string;
  constructor(k: number, v: string) {
    this.k = k;
    this.v = v;
  }
}

class Parser {
  n: number = 0;
  count(toks: Tok[]): number {
    return toks.length;
  }
  first(toks: Tok[]): string {
    return toks[0].v;
  }
  nums(xs: number[]): number {
    return xs.length;
  }
  len(s: string): number {
    return s.length;
  }
  tag(s: string): string {
    return typeof s + ":" + s;
  }
}

const p: any = new Parser();
const good: Tok[] = [new Tok(1, "a")];
console.log(p.count(good), p.first(good), p.nums([1, 2, 3]), p.len("abcd"));

for (const bad of [null, undefined, "str", 7]) {
  try {
    console.log("count", p.count(bad));
  } catch (e) {
    console.log("count", e instanceof TypeError);
  }
  try {
    console.log("first", p.first(bad));
  } catch (e) {
    console.log("first", e instanceof TypeError);
  }
  try {
    console.log("nums", p.nums(bad));
  } catch (e) {
    console.log("nums", e instanceof TypeError);
  }
  try {
    console.log("len", p.len(bad));
  } catch (e) {
    console.log("len", e instanceof TypeError);
  }
  console.log("tag", p.tag(bad));
}
console.log(p.count(good), p.len("xy"));
