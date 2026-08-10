// §10.4.4.7 step 2 — an arguments object's [[Prototype]] is
// %Object.prototype% (10.6-5-1), not Array.prototype like its Arr
// materialization would suggest; the classifier gates on
// FLAG_ARR_ARGUMENTS. The comparison target exposed a second hole:
// INLINE `Object.getPrototypeOf({})` lowered through the Obj arm's
// class-tag load — but an empty struct layout is a dynobj at
// runtime, so the load read a dynobj header field as a class tag.
// Empty-layout receivers now ride the runtime classifier.
function t() {
  console.log(Object.getPrototypeOf(arguments) === Object.getPrototypeOf({}));
  console.log(Object.getPrototypeOf(arguments) === Object.prototype);
  console.log(Object.getPrototypeOf(arguments) === Array.prototype);
}
t(1, 2);
console.log(Object.getPrototypeOf({}) === Object.prototype);
console.log(Object.getPrototypeOf([1]) === Array.prototype);
