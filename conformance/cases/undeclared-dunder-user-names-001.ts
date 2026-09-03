// `__`-prefixed identifiers are ordinary user names, not tr's own.
// An unresolvable one takes the §6.2.5.5 / §6.2.5.6 posture every
// other spelling takes: `typeof` answers "undefined", and reading or
// writing it raises a catchable ReferenceError at run time.
// (sputnik's half of test262 spells its identifiers this way
// throughout — `__ref`, `__func`, `__key`, `__in__while`.)

console.log(typeof __ref, typeof __func);

// write position — §6.2.5.6 PutValue on an unresolvable Reference
try {
  __in__while = "reached";
  console.log("no throw");
} catch (e) {
  console.log("write:", e instanceof ReferenceError, (e as ReferenceError).name);
}

// read position, and the rhs of a write is evaluated first (§13.15.2)
try {
  __func = __func;
  console.log("no throw");
} catch (e) {
  console.log("read:", e instanceof ReferenceError);
}

// unresolved capture inside a function body — the throw is at the
// read, so the function object itself is fine to create and pass around
function readsUndeclared() {
  return __key;
}
console.log(typeof readsUndeclared);
try {
  readsUndeclared();
  console.log("no throw");
} catch (e) {
  console.log("capture:", e instanceof ReferenceError);
}

// a `__` name the program DOES declare still resolves normally
const __prop = 42;
console.log(__prop, typeof __prop);

// and `typeof` on the still-unresolvable ones is unchanged after all
// that
console.log(typeof __ref, typeof __in__while);
