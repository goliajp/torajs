// RFC 20260710 C4 (chunk 751) — promotion edges: mutated captured
// PARAMS promote to a capture box (caller keeps its stake, the box
// incs its own), and Any-typed mutated captures ride the same
// machinery with the NaN-box-aware box release. Null-init and
// substr-sourced bindings pinned as already-correct.
function f(s: string): string {
  const g = () => s;
  s = s + "!";
  return g();
}
console.log(f("a"));
let v: any = "a";
const r = () => v;
v = 42;
console.log(r());
let w: any = 1;
const rw = () => w;
w = "swapped";
console.log(rw());
let s3: string | null = null;
const r3 = () => s3;
s3 = "set";
console.log(r3());
const big = "0123456789abcdef-long";
let s4 = big.substring(2, 7);
const r4 = () => s4;
s4 = s4 + "!";
console.log(r4());
