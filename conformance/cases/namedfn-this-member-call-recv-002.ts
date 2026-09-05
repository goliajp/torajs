// `.call` / `.apply` on a fn value read out of a MEMBER must bind the
// explicit thisArg (§20.2.3.3 / §20.2.3.1). The two static arms in
// `ssa_lower_call_fn_call_value` test the callee's SPELLING, and a
// field read spells neither — so the thisArg used to be evaluated for
// effect and dropped, which is only right for the no-this subset.
// A member-held cell being promoted is a RUNTIME fact, which is what
// the receiver gate is for.

function named(a: any) {
  const th = this as any
  return a + "/" + (th === undefined ? "undef" : "z" + th.z)
}

const expr = function (a: any) {
  const th = this as any
  return a + "/" + (th === undefined ? "undef" : "z" + th.z)
}

// un-annotated object literal — a nominal field read
const lit = { m: named }
console.log(lit.m.call({ z: 1 }, 7))
console.log(lit.m.apply({ z: 2 }, [8]))
// a plain call through the same field keeps the holder as receiver
console.log(lit.m(9))

// the same field with a sibling, so the field is not at offset 0
const two = { n: 1, m: named }
console.log(two.m.call({ z: 3 }, 10))

// the function-expression twin rides the identical lane
const litx = { m: expr }
console.log(litx.m.call({ z: 4 }, 11))
console.log(litx.m.apply({ z: 5 }, [12]))

// an `: any` holder already worked and must not move
const anyheld: any = { m: named }
console.log(anyheld.m.call({ z: 6 }, 13))

// a declaration that never says `this` takes the gate's plain arm
function plain(a: any, b: any) {
  return a + "|" + b
}
const litp = { m: plain }
console.log(litp.m.call({ z: 7 }, 14, 15))
console.log(litp.m.apply({ z: 8 }, [16, 17]))

// namespace statics keep the boxed dual entry
console.log(Math.max.call(null, 1, 5, 3))
console.log(Math.max.apply(null, [1, 5, 3]))
console.log(Array.from.call(Array, [1, 2]).length)
