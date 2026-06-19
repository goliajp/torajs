// ES §25.5.2.1 SerializeJSONNumber — non-finite numbers serialize
// as "null" (JSON has no NaN / Infinity literals). Pre-fix tr
// round-tripped through f64_to_str and emitted "NaN" / "Infinity"
// / "-Infinity" — invalid JSON and divergent from bun.

console.log(JSON.stringify(NaN));
console.log(JSON.stringify(Infinity));
console.log(JSON.stringify(-Infinity));

// Object field path inherits the fix via lower_json_stringify
// recursion: the Type::Obj arm walks each field, dispatching
// numeric fields to the same Type::F64 arm.
console.log(JSON.stringify({a: NaN}));
console.log(JSON.stringify({a: Infinity, b: 0, c: -Infinity}));

// Finite numbers are unaffected — sanity covers the IsFinite-true
// branch of the new CondBr.
console.log(JSON.stringify(0));
console.log(JSON.stringify(1.5));
console.log(JSON.stringify(-3.14));
console.log(JSON.stringify({a: 1.5, b: 0}));
