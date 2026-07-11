// RFC 20260711-closure-reflection chunk B — gOPD Closure arm.
// ES §20.2.4: every function object carries own `name` / `length`
// data properties { writable: false, enumerable: false,
// configurable: true }. tr answers them virtually off the anyvalue
// metadata chain (method-cell arity+interned name / bound subtract+
// "bound " prefix / fn-addr registry); a live expando entry wins.
//
// Acceptance: byte-equal with bun.

function show(d: any) {
  if (d === undefined) { console.log("undefined"); return; }
  console.log(d.value, d.writable, d.enumerable, d.configurable);
}

// 1. named fn — registry name + arity
function named(a: number, b: number) { return a + b; }
const g: any = named;
show(Object.getOwnPropertyDescriptor(g, "length"));
show(Object.getOwnPropertyDescriptor(g, "name"));

// 2. arrow via NamedEvaluation binding
const f: any = () => 1;
show(Object.getOwnPropertyDescriptor(f, "length"));
show(Object.getOwnPropertyDescriptor(f, "name"));

// 3. reified builtin method cells — receiver extraction + proto form
const n: any = 42;
const m: any = n.toFixed;
show(Object.getOwnPropertyDescriptor(m, "length"));
show(Object.getOwnPropertyDescriptor(m, "name"));
const sp: any = String.prototype.slice;
show(Object.getOwnPropertyDescriptor(sp, "length"));
show(Object.getOwnPropertyDescriptor(sp, "name"));
const dy: any = Date.prototype.getYear;
show(Object.getOwnPropertyDescriptor(dy, "name"));

// 4. bound fn — "bound " prefix + partial-arg subtract
function add3(a: number, b: number, c: number) { return a + b + c; }
const t: any = add3;
const bd: any = t.bind(null, 1);
console.log(bd(2, 3));
show(Object.getOwnPropertyDescriptor(bd, "length"));
show(Object.getOwnPropertyDescriptor(bd, "name"));

// 5. expando entry wins; unknown key stays undefined
g.custom = 7;
show(Object.getOwnPropertyDescriptor(g, "custom"));
show(Object.getOwnPropertyDescriptor(g, "zzz"));
