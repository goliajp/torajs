// §20.1.2.2 step 2 — Object.create(proto) links the proto (RFC
// 20260717-user-proto-chain knife 1). The link is identity-
// preserving; the simulation slot stays out of the own-keys answer;
// a runtime-Any null proto marks the null-prototype bit (closing
// the recorded static-literal-only residual).

const parent = { greet: "hi" };
const child: any = Object.create(parent);
console.log(Object.getPrototypeOf(child) === parent); // true
console.log(Object.keys(child).length); // 0

// two-level chain
const mid: any = Object.create(parent);
const leaf: any = Object.create(mid);
console.log(Object.getPrototypeOf(leaf) === mid); // true
console.log(Object.getPrototypeOf(Object.getPrototypeOf(leaf)) === parent); // true

// static null keeps the null-proto answer
const dict: any = Object.create(null);
console.log(Object.getPrototypeOf(dict)); // null

// runtime-Any null proto
const np: any = null;
const dict2: any = Object.create(np);
console.log(Object.getPrototypeOf(dict2)); // null

// dynobj (any-lane) parent through an Any variable
const p2: any = { tag: 1 };
const c2: any = Object.create(p2);
console.log(Object.getPrototypeOf(c2) === p2); // true

// invalid proto still throws
let caught = "";
try {
  Object.create(42 as any);
} catch (e: any) {
  caught = "yes";
}
console.log(caught); // yes
console.log("done");
