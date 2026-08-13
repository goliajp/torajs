// The eight builtin prototypes the spec gives a @@toStringTag carry
// it as a real own property; the ones it does not are unchanged.
const T: any = Symbol.toStringTag;
const ts: any = Object.prototype.toString;

function d(label: string, o: any): void {
  const x: any = Object.getOwnPropertyDescriptor(o, T);
  console.log(label + " " + (x === undefined ? "MISSING" : x.value + " w=" + x.writable + " e=" + x.enumerable + " c=" + x.configurable));
}

d("Symbol.p", Symbol.prototype);
d("Promise.p", Promise.prototype);
d("Map.p", Map.prototype);
d("Set.p", Set.prototype);
d("WeakMap.p", WeakMap.prototype);
d("WeakSet.p", WeakSet.prototype);
d("WeakRef.p", WeakRef.prototype);

// no tag on these -- their badge is the builtinTag, not a property
d("Object.p", Object.prototype);
d("Array.p", Array.prototype);
d("Number.p", Number.prototype);
d("String.p", String.prototype);
d("Boolean.p", Boolean.prototype);
d("Function.p", Function.prototype);
d("Date.p", Date.prototype);
d("RegExp.p", RegExp.prototype);
d("Error.p", Error.prototype);

// the badge on the prototype itself now comes from the property
console.log(ts.call(Map.prototype));
console.log(ts.call(Set.prototype));
console.log(ts.call(Promise.prototype));
console.log(ts.call(Symbol.prototype));
console.log(ts.call(WeakMap.prototype));

// the tagless prototypes keep the builtinTag walk
console.log(ts.call(Object.prototype));
console.log(ts.call(Number.prototype));
console.log(ts.call(Array.prototype));
console.log(ts.call(String.prototype));
console.log(ts.call(Boolean.prototype));
console.log(ts.call(Function.prototype));
console.log(ts.call(Date.prototype));
console.log(ts.call(RegExp.prototype));

// instances inherit the tag through the chain
console.log(ts.call(new Map()));
console.log(ts.call(new Set()));
console.log(ts.call(new WeakMap()));
console.log(ts.call([1, 2]));
console.log(ts.call({}));

// the property is non-enumerable, and it is the only symbol-keyed own
// property on these two (Map / Set additionally carry a real
// @@iterator -- see proto-symbol-iterator-001)
console.log(Object.keys(Map.prototype).length);
console.log(Object.getOwnPropertySymbols(WeakMap.prototype).length);
console.log(Object.getOwnPropertySymbols(Promise.prototype).length);
