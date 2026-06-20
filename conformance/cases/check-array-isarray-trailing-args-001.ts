// S267 — Array.isArray(value, ...trailing) per ES §23.1.2.2.
// Spec step 1 inspects only the first arg; trailing args
// silent-drop per ES trailing-arg ignore. tora's check.rs sig
// `vec![Type::Any] -> Boolean` rejected 1+ trailing args;
// SSA-emit's intercept routes through args[0] only — S267 adds
// a `for a in args.iter().skip(1)` eval-and-drop loop so any
// trailing expression's side effect still runs.

console.log(Array.isArray([1, 2, 3]));
console.log(Array.isArray([1, 2, 3], 99));
console.log(Array.isArray([1, 2, 3], 99, 88, 77));
console.log(Array.isArray("hi", 99));
console.log(Array.isArray(42, 77, 66));
console.log(Array.isArray({ a: 1 }, 88));

let counter = 0;
function step(): number {
  counter++;
  return counter;
}
console.log(Array.isArray([1], step(), step()));
console.log(counter);
