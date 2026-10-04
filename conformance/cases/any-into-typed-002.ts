// A `string` parameter is bound to an `any` argument as passed, on a
// direct call and through a function value alike: `null` stays `null`
// in the body (no ToString at the call), so `s.length` throws a
// TypeError and `s + "!"` reads "null!".
function len(s: string): number {
  return s.length;
}
function bang(s: string): string {
  return s + "!";
}
function kind(s: string, n: number): string {
  return typeof s + ":" + n;
}
const lenOf = (s: string): number => s.length;

class Box {
  t: string = "box";
  size(s: string): number {
    return s.length + this.t.length;
  }
  static slen(s: string): number {
    return s.length;
  }
}

const lenVal: any = len;
const bangVal: any = bang;
const b = new Box();
const vals: any[] = [null, undefined, "abc"];
for (const v of vals) {
  try {
    console.log("len", len(v));
  } catch (e) {
    console.log("len", e instanceof TypeError);
  }
  console.log("bang", bang(v));
  console.log("kind", kind(v, 1));
  try {
    console.log("lenOf", lenOf(v));
  } catch (e) {
    console.log("lenOf", e instanceof TypeError);
  }
  try {
    console.log("lenVal", lenVal(v));
  } catch (e) {
    console.log("lenVal", e instanceof TypeError);
  }
  console.log("bangVal", bangVal(v));
  try {
    console.log("size", b.size(v));
  } catch (e) {
    console.log("size", e instanceof TypeError);
  }
  try {
    console.log("slen", Box.slen(v));
  } catch (e) {
    console.log("slen", e instanceof TypeError);
  }
}
const typed: string = "typed";
console.log(len(typed), bang(typed), kind(typed, 2), b.size("ab"));
