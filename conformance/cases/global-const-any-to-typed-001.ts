// A top-level `const` with a scalar annotation becomes a module global,
// and the global lane has to make the same `any` → number / string /
// boolean crossing the fn-scope lane always made. Without it a
// Call-shaped init (not a borrow shape, so the K.4 ownership gate waved
// it through) stored the Any BOX BITS into a `str` slot and the next
// read deref'd them as a Str pointer — a silent SIGSEGV. The same
// declarations spelled `let` stay here as the paired control.

function gs(): any { return "lit" }
function gn(): any { return 7 }
function gb(): any { return false }
function pick(b: boolean): any { if (b) { return "x" } return "y" }

const s: string = gs();
console.log(s, s.length, s.toUpperCase());

const n: number = gn();
console.log(n, n + 1);

const b: boolean = gb();
console.log(b, !b);

const p: string = pick(true);
const q: string = pick(false);
console.log(p, q, p + q);

let ls: string = gs();
let ln: number = gn();
console.log(ls, ln);

// an `any` slot keeps the box (nothing to decode)
const raw: any = gs();
console.log(raw, typeof raw);

// values that were already concrete keep their old path
function cat(): any { return "a" + "b" }
const c: string = cat();
console.log(c, c.length);

// `undefined` into a sentinel-capable slot still binds the cell
// rather than ToString-ing into "undefined"
const u: string | undefined = undefined;
console.log(u);
