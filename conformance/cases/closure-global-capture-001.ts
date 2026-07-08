// closures see promoted top-level data globals through GlobalRef —
// capture lists drop global names at the construction site (l18):
// annotated refcounted top-level bindings (Str / Arr / Obj) promote to
// K.4/K.6 global slots, and closures read/mutate them like named-fn
// bodies do instead of panicking "capture not in scope".

// typed arr global mutated through closures (l18 shape)
const xs: number[] = [1];
const pusher = () => {
  for (let i = 0; i < 5000; i++) xs.push(i);
};
const reader = () => xs.length;
pusher();
console.log(reader());
console.log(xs[4000]);

// annotated str + obj globals read from top-level closures (l18c shape)
const s: string = "abc" + "d";
const slen = () => s.length;
console.log(slen());
const o: { a: number } = { a: 1 };
const geta = () => o.a;
console.log(geta());

// closure inside a named fn reaching a promoted global (l18d shape)
function viaNamedFn(): number {
  const c = () => s.length + xs[0];
  return c();
}
console.log(viaNamedFn());

// mutable number global: named fn + closure share the ONE slot (l18e)
let n: number = 10 + 5;
function bump(): void {
  n = n + 1;
}
const inc = () => {
  n = n + 100;
};
bump();
inc();
console.log(n);

// fn-local shadow of a global name still captures the LOCAL
const tag: string = "g" + "0";
function shadowCase(): string {
  const tag = "local";
  const c = () => tag;
  return c();
}
console.log(tag, shadowCase());

// nested closures pass a global straight through — no env slot at
// either level
const outer = () => {
  const inner = () => xs[0] + n;
  return inner();
};
console.log(outer());
