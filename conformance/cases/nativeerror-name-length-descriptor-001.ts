// ES §17 + §10.2.3 MakeConstructor — built-in Function objects
// (including class constructors) expose `name` and `length` with
// attributes `{writable: false, enumerable: false, configurable: true}`
// and `prototype` with `{writable: false, enumerable: false,
// configurable: false}`. Applied uniformly to built-in NativeError
// classes and user-defined classes.
//
// test262 cluster covered by this substrate:
//   built-ins/NativeErrors/{RangeError,TypeError,SyntaxError,
//     ReferenceError}/{name,length}.js  (8 cases)
//   built-ins/Error/name.js and length are implicit via `Error` below.

function report(label: string, C: any) {
  const dn = Object.getOwnPropertyDescriptor(C, "name")
  console.log(
    label + ".name",
    "value=" + (dn && dn.value),
    "W=" + (dn && dn.writable),
    "E=" + (dn && dn.enumerable),
    "C=" + (dn && dn.configurable),
  )
  const dl = Object.getOwnPropertyDescriptor(C, "length")
  console.log(
    label + ".length",
    "value=" + (dl && dl.value),
    "W=" + (dl && dl.writable),
    "E=" + (dl && dl.enumerable),
    "C=" + (dl && dl.configurable),
  )
  const dp = Object.getOwnPropertyDescriptor(C, "prototype")
  // Only report presence + attribute flags for prototype; the value
  // is a distinct object per class so a raw dump would be noisy.
  console.log(
    label + ".prototype",
    "hasValue=" + (dp && "value" in dp),
    "W=" + (dp && dp.writable),
    "E=" + (dp && dp.enumerable),
    "C=" + (dp && dp.configurable),
  )
}

// Built-in Error family — auto-injected by inject_builtin_classes.
report("Error", Error)
report("RangeError", RangeError)
report("TypeError", TypeError)
report("SyntaxError", SyntaxError)
report("ReferenceError", ReferenceError)

// User class — spec §10.2.3 MakeConstructor mandates the same shape.
class MyClass {
  x: number = 0
}
report("MyClass", MyClass)

// Values on the class must still read back correctly (attribute lock
// only affects flags, not the stored value).
console.log("RangeError.name value:", RangeError.name)
console.log("RangeError.length value:", RangeError.length)
console.log("TypeError.name value:", TypeError.name)
console.log("MyClass.name value:", MyClass.name)
