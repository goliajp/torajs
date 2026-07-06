// RFC 20260707 chunk 628 — pop/shift/unshift over a typed array
// behind a static Arr<Any> view, plus Member-expr receivers. The
// static Arr<Any> pop/shift now route through the kind-aware
// runtime helpers (typed-behind-any blocks rebox per elem kind;
// pre-fix the raw NaN-box misread SIGSEGV'd), unshift_any gained
// its typed arm (622's missed station), and the mutator receiver
// resolution admits Arr-typed Member exprs (pre-fix
// `b.arr.shift()` was a loud "unsupported member call shape").

// ident receiver, typed block behind any[] view
const nums: number[] = [30, 10, 20];
const xs: any[] = nums;
console.log(xs.pop());
console.log(xs.shift());
xs.unshift(5);
console.log(xs[0]);
console.log(nums.length);

// heap elems through the same path
const strs: string[] = ["a", "bb", "ccc"];
const ys: any[] = strs;
console.log(ys.pop());
console.log(ys.shift());
ys.unshift("z");
console.log(strs[0]);
console.log(strs.length);

// member receiver on an any[] field holding a typed block
class Box {
  arr: any[] = [];
}
const b = new Box();
const more: number[] = [7, 8, 9];
b.arr = more;
console.log(b.arr.shift());
console.log(b.arr.pop());
b.arr.unshift(1);
console.log(b.arr[0]);
console.log(more.length);

// member receiver on a typed field
class TBox {
  ns: number[] = [];
}
const t = new TBox();
t.ns = [4, 5, 6];
console.log(t.ns.pop());
console.log(t.ns.shift());
t.ns.unshift(3);
console.log(t.ns[0]);
console.log(t.ns.length);

// pure any[] blocks keep working (FLAG lane)
const mixed: any[] = [1, "two", true];
console.log(mixed.pop());
console.log(mixed.shift());
mixed.unshift(null);
console.log(mixed[0]);
console.log(mixed.length);

// empty-array pop/shift yield undefined per spec
const empty: any[] = [];
console.log(empty.pop());
console.log(empty.shift());

// (kind-mismatch push/unshift through the any view is tr's
// catchable-TypeError protocol — deliberately NOT bun semantics,
// verified probe-side, not here.)
