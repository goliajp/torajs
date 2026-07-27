// Cluster #4 follow-up — a Symbol() init toplevel let promotes to a
// K.3b global (shape inference + checker registration + assign lane
// + return-station borrow retain), so named-fn and class-method
// bodies can read it (test262 forbidden-ext family shape).
let S = Symbol("a")
function f() {
  return S
}
f()
console.log(typeof f())
console.log(String(S))
class K {
  m() {
    return S
  }
}
console.log(new K().m() === S)
function reassign() {
  S = Symbol("b")
}
reassign()
console.log(String(S))
let alias = Symbol("z")
let aliased = alias
class K2 {
  m() {
    return aliased
  }
}
console.log(typeof new K2().m())
