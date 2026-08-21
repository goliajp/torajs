// A substring view may only live in the split block that owns its
// storage; every copy OUT of a split product is an owned string.
//
// `s.split(" ")` answers one block: the array, its pointer slots, and
// the 32-byte view cells the slots point at. Copying a slot into
// another array (slice / concat / toSorted / toReversed / with /
// toSpliced / flat / spread / Array.from / Object.values / filter /
// map / flatMap / find) copied the POINTER and bumped the view cell's
// own refcount — which the inline drop path never reads. Once the
// split block was reclaimed the copy pointed at pool memory and
// printed whatever the next allocation left there (`["4","q!","xy"]`
// for `[...a, "xy"]`). The any lane boxed the view pointer the same
// way (`a.map(x => x)` through the any-callback lane). Pre-existing on
// every lane; only visible when the source array dies before the copy
// is read, and only for a heap parent (a literal parent's views
// survive by luck). Rotation 468.

// every helper builds a heap-parent split product locally and returns
// a copy; the churn afterwards reuses the freed block
function churn(): number {
  let junk: string[] = [];
  for (let i = 0; i < 64; i++) junk.push("zz" + i);
  return junk.length;
}
function viaSlice() { let s = "pear fig apple date" + "!"; let a = s.split(" "); return a.slice(1, 3); }
function viaConcatArr() { let s = "pear fig apple date" + "!"; let a = s.split(" "); return a.concat(["x" + "y"]); }
function viaConcatScalar() { let s = "pear fig apple date" + "!"; let a = s.split(" "); return a.concat("x" + "y"); }
function viaConcatView() { let s = "pear fig apple date" + "!"; let a = s.split(" "); let b = ("u v" + "?").split(" "); return a.concat(b, b[0]); }
function viaStrConcatViews() { let s = "pear fig apple date" + "!"; let parts = s.split(" "); let k: string[] = ["k" + "0"]; return k.concat(parts); }
function viaToSorted() { let s = "pear fig apple date" + "!"; let a = s.split(" "); return a.toSorted(); }
function viaToReversed() { let s = "pear fig apple date" + "!"; let a = s.split(" "); return a.toReversed(); }
function viaWith() { let s = "pear fig apple date" + "!"; let a = s.split(" "); return a.with(1, "w" + "!"); }
function viaToSpliced() { let s = "pear fig apple date" + "!"; let a = s.split(" "); return a.toSpliced(1, 1, "s" + "p"); }
function viaFlat() { let s = "pear fig apple date" + "!"; let a = s.split(" "); return a.flat(); }
function viaSpread() { let s = "pear fig apple date" + "!"; let a = s.split(" "); return [...a, "x" + "y"]; }
function viaFrom() { let s = "pear fig apple date" + "!"; let a = s.split(" "); return Array.from(a); }
function viaValues() { let s = "pear fig apple date" + "!"; let a = s.split(" "); return Object.values(a); }
function viaFilter() { let s = "pear fig apple date" + "!"; let a = s.split(" "); return a.filter(x => x !== "fig"); }
function viaMap() { let s = "pear fig apple date" + "!"; let a = s.split(" "); return a.map(x => x); }
function viaMapAnn() { let s = "pear fig apple date" + "!"; let a = s.split(" "); return a.map((x: string): string => x); }
function viaFlatMap() { let s = "pear fig apple date" + "!"; let a = s.split(" "); return a.flatMap(x => x.split("a")); }
function viaFind() { let s = "pear fig apple date" + "!"; let a = s.split(" "); return a.find(x => x.startsWith("a")); }
function viaAnyBox() { let s = "pear fig apple date" + "!"; let a = s.split(" "); let v: any = a[2]; let w: any[] = [a[0], a[3]]; return [v, w]; }

const r = [
  viaSlice(), viaConcatArr(), viaConcatScalar(), viaConcatView(), viaStrConcatViews(),
  viaToSorted(), viaToReversed(), viaWith(), viaToSpliced(), viaFlat(), viaSpread(),
  viaFrom(), viaValues(), viaFilter(), viaMap(), viaMapAnn(), viaFlatMap(),
];
const found = viaFind();
const boxed = viaAnyBox();
churn();
for (const x of r) console.log(JSON.stringify(x));
console.log(found, JSON.stringify(boxed));
// the copies are ordinary owned strings: they take owned writes
const c = viaSlice();
c.push("z" + "w");
c.sort();
console.log(c.join("+"), c.length);
// an index store of a view into an owned-string array stores a copy
function viaIndexStore() { let s = "pq" + "!"; let a = s.split(""); let out: string[] = ["x" + "y"]; out[0] = a[1]; out.push(s[0]); return out; }
const st = viaIndexStore();
churn();
console.log(JSON.stringify(st));
