// Object.values / Object.entries on any-typed receivers: full
// ToObject dispatch (ES §20.1.2.22 / §20.1.2.5), mirroring the
// Object.keys chooser's receiver taxonomy. Formerly every non-DynObj
// non-struct receiver threw "Object reflection on a non-struct any
// value is not yet supported".

// string receiver — per-code-unit values / [idx, ch] entries
const s: any = "hello world";
console.log(Object.values(s).length, Object.entries(s)[10]);
const wide: any = "汉x";
console.log(Object.values(wide), Object.entries(wide));

// array receiver — kind-aware element walk + expando tail
const xs: any = [1, "two", null];
(xs as any).tag = "ex";
console.log(Object.values(xs), Object.entries(xs));
const mixed: any = [true, 1.5, "s"];
console.log(Object.entries(mixed));

// closure receiver — expando only (name/length are non-enumerable)
const f: any = (x: number) => x;
f.note = 7;
console.log(Object.values(f), Object.entries(f));

// empty receivers
const empty0: number[] = [];
const empty: any = empty0;
console.log(Object.values(empty), Object.entries(empty), Object.values("" as any));

// primitive receivers — ToObject wrapper has no own enumerable keys
console.log(Object.values(42 as any), Object.entries(42 as any));
console.log(Object.values(true as any));

// dynobj receiver + fromEntries round-trip (regression)
const o: any = { a: 1, b: "x" };
console.log(Object.values(o), Object.entries(o));
console.log(Object.fromEntries(Object.entries(o)));

// null / undefined — catchable TypeError per ToObject
try { Object.values(null as any); } catch (e: any) { console.log("null: caught"); }
try { Object.entries(undefined as any); } catch (e: any) { console.log("undef: caught"); }
