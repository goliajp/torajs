// A split product that the program writes owned strings INTO is
// materialized at its binding and typed as an owned-string array.
//
// `s.split(" ")` answers an array of substring VIEWS; an `Arr<Substr>`
// slot cannot take an owned string (a view cannot be minted from a
// fresh string), and every reader decodes its slots by the view layout.
// `a.push("z"+"w"); a.join("+")` therefore SIGSEGV'd, as did unshift /
// splice / fill / `a[i] = v`, on `let` and `const` alike — plan-state
// 467-01, pre-existing. The binding lane now asks a whole-scope census
// (`analyze_let_owned_writes`): a product something in scope writes
// into is materialized in place — every view becomes an owned string —
// and the binding is `Arr<Str>`, where the mutators store any shape.
// Rotation 468.

// the three judgments from the plan-state entry
let a: string[] = "p q r".split(" ");
a.push("z" + "w");
console.log(a.join("+"));
let a2 = "p q r".split(" ");
console.log(a2.concat(["s"]).join("-"));
console.log("c b a".split(" ").sort().join(""));

// every owned-write shape, on a heap parent, read back on every path
function viaPush() { let s = "p q r" + "!"; let x = s.split(" "); x.push("z" + "w"); return x; }
function viaUnshift() { let s = "p q r" + "!"; let x = s.split(" "); x.unshift("z" + "w"); return x; }
function viaSplice() { let s = "p q r" + "!"; let x = s.split(" "); x.splice(1, 0, "z" + "w"); return x; }
function viaFill() { let s = "p q r" + "!"; let x = s.split(" "); x.fill("z" + "w", 1); return x; }
function viaIndex() { let s = "p q r" + "!"; let x = s.split(" "); x[1] = "z" + "w"; return x; }
function viaCopyWithin() { let s = "p q r" + "!"; let x = s.split(" "); x.copyWithin(0, 1); return x; }
function viaPushView() { let s = "p q r" + "!"; let x = s.split(" "); let y = ("u v" + "?").split(" "); x.push(y[0]); x[0] = y[1]; return x; }
function viaConstPush() { const s = "p q r" + "!"; const x = s.split(" "); x.push("z" + "w"); x.sort(); return x; }
const results = [viaPush(), viaUnshift(), viaSplice(), viaFill(), viaIndex(), viaCopyWithin(), viaPushView(), viaConstPush()];
let junk: string[] = [];
for (let i = 0; i < 64; i++) junk.push("zz" + i);
for (const r of results) {
  console.log(JSON.stringify(r), r.join("|"), r.length, r.indexOf("zw"), r.includes("q"), r.at(-1));
}

// reads that follow a write: for-of, sort, reverse, map, slice
let w = "c b a".split(" ");
w.push("d" + "");
w.sort();
for (const x of w) console.log(x);
console.log(w.reverse().join(""), w.map(x => x + "!").join(""), w.slice(1).join(""));

// a write through a hoisted function declared before the binding
function poke() { h.push("h" + "!"); }
let h = "p q".split(" ");
poke();
console.log(h.join("+"), h.length);

// top-level const with a write from a function body (not promoted)
const tl = "x y".split(" ");
function addTl() { tl.push("z" + "z"); }
addTl();
console.log(tl.join(","), tl.length);

// a product nobody writes into stays views: the same reads still agree
const ro = "m n o".split(" ");
console.log(ro.join("."), ro.indexOf("n"), ro.at(0), JSON.stringify(ro));

// the whole array escaping as a bare value: a call argument, another
// binding, an assignment value, a literal element, an any binding —
// each receiver is typed by its own annotation and reads owned strings
function addx(xs: string[]) { xs.push("x" + "!"); }
function rd(xs: string[]): string { return xs.join("-") + xs[0].length + JSON.stringify(xs); }
function viaArg() { let s = "p q" + "!"; let x = s.split(" "); addx(x); return rd(x); }
function viaAlias() { let s = "p q" + "!"; let x = s.split(" "); let y = x; y.push("z" + "w"); return x.join("-") + "/" + y.join("-"); }
function viaAny() { let s = "p q" + "!"; let x = s.split(" "); let v: any = x; let out: any[] = []; for (const e of v) out.push(e); return JSON.stringify(out) + v[1]; }
function viaLiteral() { let s = "p q" + "!"; let x = s.split(" "); let wrapped: string[][] = [x]; wrapped[0].push("l" + "!"); return JSON.stringify(wrapped); }
class Holder { xs: string[] = []; }
function viaField() { let s = "p q" + "!"; let x = s.split(" "); let hh = new Holder(); hh.xs = x; hh.xs.push("f" + "!"); return hh.xs.join("-") + "/" + x.length; }
const esc = [viaArg(), viaAlias(), viaAny(), viaLiteral(), viaField()];
for (let i = 0; i < 64; i++) junk.push("zz" + i);
for (const e of esc) console.log(e);
