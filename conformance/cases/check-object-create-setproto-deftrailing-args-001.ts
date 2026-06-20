// S269 — Object.{create,setPrototypeOf,defineProperties}
// trailing-arg ignore per ES §20.1.2.{1,5,21}. Each method's
// real spec arity is 1 or 2; trailing args silent-drop per
// generic ES trailing-arg ignore. tora's check.rs fixed sigs
// (`vec![Type::Any]` / `vec![Type::Any, Type::Any]`) rejected
// the next arg; SSA-emit for all three already evals + drops
// args beyond the first useful slot via `for a in args`
// (create) or `for a in args.iter().skip(1)` (setPrototypeOf
// / defineProperties), so the runtime path stays correct
// — S269 widens checktime to accept the matching arity floor.

console.log(typeof Object.create({}, {}));
console.log(typeof Object.create({}, {}, 99));
console.log(typeof Object.create(null, undefined, 77, 66));

const o1 = Object.setPrototypeOf({ x: 1 }, null, 99);
console.log(typeof o1);
const o2 = Object.setPrototypeOf({ y: 2 }, null, 88, 77);
console.log(typeof o2);

Object.defineProperties({ a: 1 }, { a: { value: 99 } }, 99);
Object.defineProperties({ b: 2 }, { b: { value: 99 } }, 99, 88, 77);
console.log("done");
