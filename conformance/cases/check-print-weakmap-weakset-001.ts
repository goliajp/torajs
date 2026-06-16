// S130-2 narrow: console.log(WeakMap) / console.log(WeakSet) prints
// the fixed `WeakMap {}` / `WeakSet {}` form (bun default inspect).
// Pre-fix the Tag::Weak* arm fell into the `[object]` placeholder
// inside both inspect dispatchers (any.rs trailing '\n' + tag_dispatch
// inline). WeakMap / WeakSet are non-enumerable per spec §24.4 / §24.5
// (no forEach / iterators), so bun prints the fixed empty-shape form
// regardless of internal entry count.
//
// Acceptance covers the print dispatcher arm only — entry-bearing
// receivers are exercised by check-class-field-weakmap-weakset-001
// (string V, avoids the number-value-as-Ptr independent wedge).
const wm = new WeakMap<object, string>();
const ws = new WeakSet<object>();

console.log(wm);
console.log(ws);
console.log(typeof wm, typeof ws);
