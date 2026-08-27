// A rest parameter binds the empty array when there is nothing left
// to put in it (§10.2.11), and a fixed parameter the call did not
// reach binds undefined — or its own default. The pass that packs a
// variadic call site used to walk away from a call that stopped short
// of the fixed prefix, and nothing downstream could line the
// arguments up afterwards, so `g()` on `function g(x, ...r)` was
// refused for want of an argument the language never asks for.
function g(x, ...r) {
  return "g:" + x + ":" + r.length
}
console.log(g(), g(1), g(1, 2), g(1, 2, 3))

// more than one fixed parameter, and the call reaching only some
function h(x, y, ...r) {
  return "h:" + x + ":" + y + ":" + r.length
}
console.log(h(), h(1), h(1, 2), h(1, 2, 3, 4))

// the missing parameter takes its OWN default, not undefined
function d(x = 5, y = "b", ...r) {
  return "d:" + x + ":" + y + ":" + r.length
}
console.log(d(), d(1), d(1, "z"), d(1, "z", 9))

// a class method reached the same way
class C {
  m(x = 7, ...r) {
    return "m:" + x + ":" + r.length
  }
  n(x, ...r) {
    return "n:" + x + ":" + r.length
  }
}
const c = new C()
console.log(c.m(), c.m(1), c.m(1, 2))
console.log(c.n(), c.n(1), c.n(1, 2))

// nothing fixed at all still answers the empty tail
function k(...r) {
  return "k:" + r.length
}
console.log(k(), k(1), k(1, 2))

// an explicit undefined at a defaulted position is still the default
console.log(d(undefined), d(undefined, undefined, 1))
