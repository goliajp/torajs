// super[k]: the key expression runs before the home object's
// prototype is read, so a key that unlinks the chain makes the read
// throw; super.m(arg) and super["p"] = v fix their base first.
class Base {
  method(): number {
    return 1
  }
}
class Derived extends Base {
  count = 0
  elem(): unknown {
    return super[ruin()]
  }
  prop(): number {
    return super.method(ruin())
  }
  assign(): void {
    super["p"] = ruin()
  }
}
function ruin(): number {
  Object.setPrototypeOf(Derived.prototype, null)
  return 5
}
function reset(): void {
  Object.setPrototypeOf(Derived.prototype, Base.prototype)
}
const d = new Derived()
try {
  d.elem()
  console.log("no throw")
} catch (e) {
  console.log((e as Error).constructor === TypeError)
}
reset()
console.log(d.prop())
reset()
d.assign()
console.log((d as any).p)
reset()
let k = 0
class C2 extends Base {
  read(): unknown {
    return super[(k = k + 1, "method")]
  }
}
console.log(typeof new C2().read(), k)
