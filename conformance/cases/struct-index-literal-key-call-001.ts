// `check_type_of_index` resolves a struct receiver keyed by a
// compile-time literal through the member checker (`g[0]` is `g."0"`
// per §7.1.19 ToPropertyKey). The CALL side had no matching arm: the
// read answered a function and the call crashed the compiler with
// "unsupported callee form: Index", while the same program spelled
// `(c as any)[1]()` ran.
class Numbered {
  [1]() {
    return "one";
  }
  ["two-part"]() {
    return "hyphen";
  }
  named() {
    return "named";
  }
}
const c = new Numbered();
console.log(c[1]());
console.log(new Numbered()[1]());
console.log(c["two-part"]());
console.log(c["named"]());
console.log(typeof c[1]);

const plain = { 0: () => "zero", "a-b": () => "dash" };
console.log(plain[0](), plain["a-b"]());

// The array element call keeps its own lane.
const fns = [() => "first", () => "second"];
console.log(fns[1]());
