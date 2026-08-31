// Rotation 543 — the Object.values / Object.entries lanes read the
// receiver without consuming it, and neither had the `arg_raw`
// release its sibling Object.keys carries. An owned argument temp
// (object literal, call result, `as` cast) therefore had no release
// site at all: 200k of `Object.values({a: 1})` peaked at 14.35 MB
// RSS and `Object.entries({a: 1})` at 14.75 MB, against 1.51 MB for
// the same loop over a bound receiver.
//
// The leak itself is invisible to a stdout gate. What this pins is
// the other side of the release: the result is still correct, and a
// bound receiver is still alive after the call (an over-release
// would take the binding's own reference).
console.log(Object.values({ a: 1, b: 2 }));
console.log(Object.entries({ a: 1, b: 2 }));

const o = { a: 3, b: 4 };
console.log(Object.values(o), Object.entries(o), o.a, o.b);
console.log(Object.values(o), Object.entries(o));

console.log(Object.values([7, 8]), Object.entries([7, 8]));

function mk(): string {
  return "ab";
}
console.log(Object.values(mk()), Object.entries(mk()));
