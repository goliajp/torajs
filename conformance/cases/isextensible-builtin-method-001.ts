// ES §7.3.15 Object.isExtensible / §7.3.16 Object.isSealed —
// reified builtin method cells are extensible by default. Regression
// fix: previously tr's __torajs_obj_is_extensible treated
// FLAG_STATIC_LITERAL as non-extensible (conflating rc-immortality
// with [[Extensible]]), so every reified Set/Array/Object/Function
// prototype method value answered false, tripping test262
// Set/prototype/{difference,union,intersection,isSubsetOf,
// isSupersetOf,isDisjointFrom,symmetricDifference}/builtins.js
// (assert Object.isExtensible(Set.prototype.difference) === true).
// Fix moves the primitive-in-spec (Str/Symbol/BigInt cell) check up
// to the anyv layer so obj_is_extensible only reads the standard
// NON_EXTENSIBLE bit.

// --- reified builtin method cells (Tag::Closure + STATIC_LITERAL) ---

// Set.prototype.<set-method> — the test262 case shape.
console.log("Set.prototype.difference ext:", Object.isExtensible(Set.prototype.difference));
console.log("Set.prototype.union      ext:", Object.isExtensible(Set.prototype.union));
console.log("Set.prototype.add        ext:", Object.isExtensible(Set.prototype.add));

// Array.prototype methods — same reified-cell path.
console.log("Array.prototype.map  ext:", Object.isExtensible(Array.prototype.map));
console.log("Array.prototype.push ext:", Object.isExtensible(Array.prototype.push));

// Object.prototype methods — same reified-cell path via a different
// proto tag.
console.log(
  "Object.prototype.hasOwnProperty ext:",
  Object.isExtensible(Object.prototype.hasOwnProperty),
);

// isSealed sibling — extensible → not sealed.
console.log(
  "Set.prototype.difference sealed:",
  Object.isSealed(Set.prototype.difference),
);
console.log(
  "Array.prototype.map sealed:",
  Object.isSealed(Array.prototype.map),
);

// --- primitive-in-spec heap cells (Str / Symbol / BigInt) ---

// Str cell — spec §7.3.15 step 1 "Type(O) is not Object → false".
console.log('Object.isExtensible("foo"):', Object.isExtensible("foo"));
console.log('Object.isSealed("foo"):', Object.isSealed("foo"));

// BigInt cell — same rule.
console.log("Object.isExtensible(1n):", Object.isExtensible(1n));
console.log("Object.isSealed(1n):", Object.isSealed(1n));

// --- primitive imms (nan-boxed, non-cell) ---

console.log("Object.isExtensible(42):", Object.isExtensible(42));
console.log("Object.isExtensible(true):", Object.isExtensible(true));
console.log("Object.isExtensible(null):", Object.isExtensible(null));
console.log("Object.isExtensible(undefined):", Object.isExtensible(undefined));
console.log("Object.isSealed(42):", Object.isSealed(42));
console.log("Object.isSealed(null):", Object.isSealed(null));

// --- ordinary DynObj — regression witness (existing behavior preserved) ---

console.log("Object.isExtensible({}):", Object.isExtensible({}));
console.log("Object.isSealed({}):", Object.isSealed({}));

var frozen: any = Object.preventExtensions({ x: 1 });
console.log("Object.isExtensible(prevented {}):", Object.isExtensible(frozen));

var sealed: any = Object.seal({ x: 1 });
console.log("Object.isSealed(sealed {}):", Object.isSealed(sealed));

// --- prototype objects themselves stay extensible ---

console.log("Object.isExtensible(Set.prototype):", Object.isExtensible(Set.prototype));
console.log("Object.isExtensible(Array.prototype):", Object.isExtensible(Array.prototype));
