// Assignment is the third face of the split-product rule (rotation
// 468: a view only lives in the split block that owns it; any copy out
// is an owned string, any array that holds the copies is `Arr<Str>`).
//
// Two shapes used to be a loud "assignment to `a` mismatch — slot is
// Arr(0) but value is Arr(1)" (plan-state 468-02): a split product
// rebound to another array, and an owned-string array rebound to a
// fresh split product. The census now lists a binding that is itself
// reassigned — its slot must take whatever the program stores later —
// and the assignment boundary converts a view array entering an
// owned-string slot (a fresh product is materialized in place, an
// alias is copied out and adopted). Rotation 469.

// a split product rebound to an array literal, then written into
let a = "p q".split(" ");
a = ["x" + "y"];
a.push("z" + "!");
console.log(a.join("+"), a.length);

// an owned-string array rebound to a fresh split product (heap parent)
let b: string[] = [];
b = ("p q r" + "!").split(" ");
b.push("s" + "t");
console.log(b.join("-"), b.length, b.indexOf("q"));

// rebound in a loop: the product of each iteration replaces the last
let parts: string[] = [];
let total = 0;
for (let i = 0; i < 5; i++) {
  parts = ("a b c " + i).split(" ");
  total = total + parts.length;
}
console.log(parts.join("|"), total);

// rebound through an alias: both names see the same array
let src = ("u v" + "!").split(" ");
let dst: string[] = [];
dst = src;
dst.push("w" + "!");
console.log(src.join("|"), dst.join("|"), src.length === dst.length);

// a product rebound to another product
let c = ("m n" + "!").split(" ");
c = ("o p q" + "?").split(" ");
c[0] = "r" + "s";
console.log(c.join(","), c.length);

// top-level binding rebound from a function body (the promoted lane)
let g: string[] = [];
function setG() { g = ("a b" + "?").split(" "); }
setG();
g.push("c" + "d");
console.log(g.join(","), g.length);

// the sources die, the pool churns, the rebound arrays are read back
function make(tag: string): string[] {
  let out: string[] = [];
  let s = "k l m " + tag;
  out = s.split(" ");
  out.push("n" + tag);
  return out;
}
const kept = [make("1"), make("2"), make("3")];
let junk: string[] = [];
for (let i = 0; i < 64; i++) junk.push("zz" + i);
for (const r of kept) console.log(JSON.stringify(r), r.join("|"), r.at(-1));

// a product nobody rebinds or writes stays views: the same reads agree
const ro = "m n o".split(" ");
console.log(ro.join("."), ro.indexOf("n"), ro.at(0), JSON.stringify(ro));
