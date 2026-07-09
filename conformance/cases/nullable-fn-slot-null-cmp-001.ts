// chunk 738 — nullable fn-typed slots: null comparison + reassign-over-null drop
function nf(): string {
  return "named";
}

// immutable + null init — FnSig slot vs null (strict-eq family fold)
const f: (() => string) | null = null;
console.log(f === null);
console.log(f !== null);

// immutable + named-fn init (fn_addr_let) — FnSig slot, real pointer cmp
const g: (() => string) | null = nf;
console.log(g === null);
if (g !== null) {
  console.log(g());
}

// immutable + arrow init — Closure repr
const a: (() => string) | null = () => "arrow";
console.log(a === null);
if (a !== null) {
  console.log(a());
}

// mutable: closure → null → closure (drop-old over the null sentinel)
let h: (() => string) | null = () => "one";
console.log(h === null);
h = null;
console.log(h === null);
h = () => "two";
if (h !== null) {
  console.log(h());
}

// mutable: null init → closure reassign
let k: (() => string) | null = null;
k = () => "kk";
console.log(k !== null);
if (k !== null) {
  console.log(k());
}

// fn-local scope: nullable closure slot, scope-close drop over null
function local(): void {
  let m: (() => string) | null = null;
  m = () => "mm";
  m = null;
}
local();
console.log("done");
