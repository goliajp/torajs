// A direct call with `any` arguments for `string` parameters runs the
// typed body when every such argument is a string and binds the value
// as passed otherwise, alternating call by call.
function pair(a: string, n: number, b: string): string {
  return a + n + b;
}
function size(s: string): number {
  return s.length;
}
function note(s: string): void {
  console.log("note", typeof s, s);
}

const vals: any[] = ["ab", null, "a longer string", undefined, "", "cde".repeat(4)];
let total = 0;
for (const v of vals) {
  console.log("pair", pair(v, 1, "z"), pair("y", 2, v));
  try {
    total += size(v);
    console.log("size", size(v));
  } catch (e) {
    console.log("size", e instanceof TypeError);
  }
  note(v);
}
console.log("total", total);
