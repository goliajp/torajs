// Phase K.4 — refcount-typed (Str) globals. Top-level
// `const NAME: string = <fresh-heap init>` becomes a real LLVM
// pointer-shaped data slot; the heap StrRepr* is stored at main
// entry and dropped at fall-through main exit (verified leak-free
// via `leaks --atExit` on the AOT binary — minimum-form smoke test
// in the K.4 commit body).
//
// K.4 limits exercised:
//   - init must be a fresh-heap value (function-call return here)
//     — not a borrow-shaped Ident / Member / Index. Borrow-init for
//     refcount globals would need an extra `rc_inc` and is deferred.
//   - the global is read from a named-fn body — read is borrow-
//     shaped (Load with no inc), so subsequent uses don't double-
//     drop the slot.
//   - no mutable assign; assignment to a refcount global is rejected
//     loudly (mutable refcount globals are a follow-up).

function buildName(): string {
  return "ToraJS";
}

function buildTag(): string {
  return "lib";
}

const NAME: string = buildName();
const TAG: string = buildTag();

function nameLength(): number {
  return NAME.length;
}

function tagFirst(): number {
  return TAG.charCodeAt(0);
}

function check(): number {
  if (NAME !== "ToraJS") { throw "#1: NAME"; }
  if (TAG !== "lib") { throw "#2: TAG"; }
  if (NAME.length !== 6) { throw "#3: NAME.length direct"; }
  if (nameLength() !== 6) { throw "#4: NAME.length via fn"; }
  if (tagFirst() !== 108) { throw "#5: TAG.charCodeAt(0) === 'l'"; }
  return 0;
}
console.log(check());
