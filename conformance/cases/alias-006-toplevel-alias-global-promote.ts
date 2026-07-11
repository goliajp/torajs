// chunk 810 — un-annotated top-level alias of another top-level
// binding promotes to a data-global slot when a named fn reads it
// (infer_toplevel_slot_shape Ident arm resolves the upstream LetDecl
// init shape, chain-depth capped).
const base = 10;
const alias = base;
function g(): number { return alias + 1 }
console.log(g());

// chain alias
const a1 = 7;
const b1 = a1;
const c1 = b1;
function h(): number { return c1 * 2 }
console.log(h());

// f64 upstream
const fbase = 1.5;
const falias = fbase;
function gf(): number { return falias + 0.5 }
console.log(gf());

// string upstream
const sbase = "hi";
const salias = sbase;
function gs(): string { return salias + "!" }
console.log(gs());

// annotated upstream maps through the simple-ann table
const nbase: number = 10;
const nalias = nbase;
function gn(): number { return nalias + 1 }
console.log(gn());

// mutable upstream widened to f64 before the alias copies it —
// num_width slot-to-slot propagation must mark the alias slot f64
let wbase = 10;
wbase = 1.5;
const walias = wbase;
function gw(): number { return walias + 1 }
console.log(gw());

// negated alias + un-annotated reader fn
const mbase = 10;
const malias = -mbase;
function gm() { return malias + 1 }
console.log(gm());
