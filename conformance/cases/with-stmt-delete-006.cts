// ES §14.11 + §13.5.1 — `delete <bare name>` inside a `with` body.
// Sloppy goal only, so this fixture is `.cts` (`delete` on a bare name
// is a SyntaxError in strict code, and must stay one).
//
// §13.5.1.2 evaluates the reference BEFORE deciding anything, and
// §9.1.1.2 makes a reference the with object supplies a PROPERTY
// reference. So the object arm has to really remove `o.x`, where the
// fall-through only answers a boolean.
//
// The bare-name triage folds each site to a constant from what the
// program declares. It used to run before this desugar, which meant
// `with (o) { delete x }` answered `true` and removed nothing — the
// property was still there afterwards. It now runs after, and what
// reaches it is the fall-through arm alone.
//
// What each block pins:
//   - a property the object carries is actually removed, and the
//     answer is the [[Delete]] result (true for a configurable one);
//   - a name the object does NOT carry falls through to §13.5.1.2:
//     false for a declared binding, true for an unresolvable one;
//   - the membership test is per reference, so deleting the same name
//     twice answers differently the second time;
//   - `delete` inside a nested function body works through the
//     captured binding, at call time.

var o: any = { x: 1, y: 2 };
var declared = "declared";

with (o) {
  // carried by the object -> a property delete, and it really goes
  console.log(delete x);
  console.log("x" in o, o.y);
  // the same name again: the object no longer carries it, so this is
  // the fall-through, and `x` is not declared anywhere
  console.log(delete x);
  // a declared binding is non-configurable per §9.1.1.1.7
  console.log(delete declared);
  // never declared, never on the object -> §13.5.1.2 step 3.a
  console.log(delete neverHeardOf);
}

console.log(declared);

// NOT pinned here: a NON-CONFIGURABLE own property, which §13.5.1.2
// step 5 answers `false` for under the sloppy goal and only throws
// under the strict one. tr throws under both, which is a member-delete
// residual independent of `with` (`delete f.k` alone reproduces it in
// a plain `.cts`). Pinning it here would make this fixture fail for a
// reason that has nothing to do with §14.11.

// Through a nested function, resolved when it is CALLED.
var later: any = { p: "gone-later" };
var drop: any = null;
with (later) {
  drop = function (): boolean {
    return delete p;
  };
}
console.log("p" in later);
console.log(drop());
console.log("p" in later);
// The object no longer carries it, so the same closure now falls
// through to an unresolvable name.
console.log(drop());
