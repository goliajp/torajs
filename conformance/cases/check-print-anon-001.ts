// W-J Phase A1 follow-up acceptance — anonymous ObjectLit console.log
// parity. Pre-A1-follow-up: tr printed `{}` because Pass 2-interned
// `{x:1,y:2}` sids missed the Pass 1.5 `anon_sid_to_tag` snapshot,
// stamping `class_tag@+8 = 0`. `struct_print.rs` Tag::Obj walker then
// saw `__torajs_struct_layout_lookup(0) → NULL` → `field_count → 0`
// → emitted the prefix-less single-line `{}`.
//
// A1 follow-up routes every Pass 2 stamp through the `AnonStampPool`
// (RefCell) — fresh sids get a contiguous tag past the snapshot's
// high-water mark + `lower_module` walks `pool.fresh_sids()` after
// Pass 2 to append matching `ClassLayoutMeta` rows. `cmd_build` skips
// `__anon_struct_*` class_names so the walker stays prefix-less for
// anonymous shapes (bun matches the no-name `{…}` form for
// `{a:1}` / `{x:1,y:2}` style literals).
const a = { x: 1, y: 2 };
console.log(a);

const b = { name: "foo", v: 42, on: true };
console.log(b);

function check() {
  const c = { z: 3.14, label: "inner" };
  console.log(c);
}
check();
