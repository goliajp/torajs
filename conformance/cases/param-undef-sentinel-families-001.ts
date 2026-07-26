// A parameter handed an out-of-range read (or a `find` miss, or a
// `pop` off an empty array) carries that answer's sentinel, exactly
// like a binding initialized from the same shape. The binding is
// recorded at its let-decl; a parameter's value arrives from a caller
// lowered separately, so the callee has to be told — and it was only
// being told for `number`, though the collector names the parameter
// whatever the family. `string`, `Substr` and the refcounted-pointer
// families each get recorded in the set their own consumers read.
function line(tag: string, f: () => unknown) {
  try { console.log(tag, f()); } catch (e) { console.log(tag, "THREW", (e as Error).name); }
}
const ss: string[] = ["ab"];
const ds: Date[] = [new Date(0)];
const os: { v: number }[] = [{ v: 1 }];
const ns: number[] = [1.5];

function s_typeof(x: string) { return typeof x; }
function s_eq(x: string) { return x === undefined; }
function s_print(x: string) { console.log("  s_print", x); return 0; }
function s_len(x: string) { return x.length; }
function s_truthy(x: string) { return x ? "yes" : "no"; }
function d_typeof(x: Date) { return typeof x; }
function d_eq(x: Date) { return x === undefined; }
function d_print(x: Date) { console.log("  d_print", x); return 0; }
function d_member(x: Date) { return x.getTime(); }
function o_typeof(x: { v: number }) { return typeof x; }
function o_member(x: { v: number }) { return x.v; }
function o_eq(x: { v: number }) { return x === undefined; }
function n_typeof(x: number) { return typeof x; }

line("s-typeof", () => s_typeof(ss[7]));
line("s-eq", () => s_eq(ss[7]));
line("s-print", () => s_print(ss[7]));
line("s-len", () => s_len(ss[7]));
line("s-truthy", () => s_truthy(ss[7]));
line("s-live", () => s_typeof(ss[0]));
line("s-live-len", () => s_len(ss[0]));
line("d-typeof", () => d_typeof(ds[7]));
line("d-eq", () => d_eq(ds[7]));
line("d-print", () => d_print(ds[7]));
line("d-member", () => d_member(ds[7]));
line("d-live", () => d_member(ds[0]));
line("o-typeof", () => o_typeof(os[7]));
line("o-eq", () => o_eq(os[7]));
line("o-member", () => o_member(os[7]));
line("o-live", () => o_member(os[0]));
line("n-typeof", () => n_typeof(ns[7]));
line("n-live", () => n_typeof(ns[0]));
