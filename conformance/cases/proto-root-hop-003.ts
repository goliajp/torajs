// %Object.prototype% patch reached by the CALL lane of the two
// property-carrying shapes (521-06). Both the dynobj arm and the
// static-layout struct arm claim their receiver and end at the
// inherited-surface fallback, so the dispatcher tail's patch consult
// never ran for them: `Object.prototype.mm = f; ({}).mm()` threw
// while `({} as any).mm` already read the function.
(Object.prototype as any).mm = function () {
  return 9
}

const plain: any = { x: 1 }
console.log(plain.mm())

class C {
  x = 1
}
const inst: any = new C()
console.log(inst.mm())

// A chain that already had a user parent keeps working — the parent's
// full [[Get]] recursion was always the path that reached the root.
const child: any = Object.create({ a: 1 })
console.log(child.mm())

// A null-prototype object is off the chain entirely.
console.log(typeof Object.create(null).mm)

// An own entry storing undefined shadows the patch: resolved, not
// callable.
const shadowed: any = { mm: undefined }
try {
  shadowed.mm()
} catch (e: any) {
  console.log("shadowed", e instanceof TypeError)
}

// A non-callable patch is the same TypeError, not a fall-through to
// the builtin surface.
;(Object.prototype as any).nn = 5
try {
  plain.nn()
} catch (e: any) {
  console.log("noncallable", e instanceof TypeError)
}
