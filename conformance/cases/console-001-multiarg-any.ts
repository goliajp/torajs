// P-CONSOLE follow-up — console.log(literal, e) where e is Type::Any
// (the default catch-binding shape). Pre-fix tora panicked at
// ssa_lower "multi-arg coercion of type Any not supported". Runtime
// helper `__torajs_any_to_str` already had full tag-dispatch (Number
// / String / Boolean / null / undef / heap pointers + Str rc-inc);
// just needed to export it + add a coerce_to_str arm that splits the
// Any into (tag, value) via the existing unbox intrinsics and routes
// through it.
//
// Coverage: catch-bound primitive (string / number / bool / null /
// undefined) — the dominant shapes thrown by user code + spec
// runtime errors. Heap-thrown objects ToString to "[object]"
// placeholder per the runtime helper's existing fallback.

// Path A — string reason
try {
  throw 'bad'
} catch (e) {
  console.log('caught', e)
}

// Path B — number reason
try {
  throw 42
} catch (e) {
  console.log('caught', e)
}

// Path C — boolean reason
try {
  throw true
} catch (e) {
  console.log('caught', e)
}

// Path D — null reason
try {
  throw null
} catch (e) {
  console.log('caught', e)
}

// Path E — Promise.reject await-throws (P10.4-A2) caught by typed param
try {
  let p: Promise<number> = Promise.reject(7)
  let v = await p
  console.log('no-throw', v)
} catch (e) {
  console.log('caught-reject', e)
}
