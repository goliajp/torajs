// A namespace whose exports are `null` and an uninitialized `var`.
//
// The namespace object now lowers through the dynobj lane, and that
// lane refused a field whose SSA type is Ptr — `ssa-lower: dynobj
// init unsupported field type Ptr`. The folded `null` CONSTANT had an
// arm; the same null arriving through a SLOT did not, and
// `export default null` is exactly that: the resolver's synthetic
// binding, read back by the namespace field.
import * as ns from "./lib";

console.log("later" in ns, "default" in ns);
console.log(ns.default, ns.default === null);
console.log(ns.later, typeof ns.later);
console.log(Object.getOwnPropertyNames(ns).join(","));
