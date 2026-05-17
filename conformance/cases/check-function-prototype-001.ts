// P4.4 — Function.prototype.bind / call / apply via AST-level
// desugar at `desugar_function_prototype_methods`. tora's typed-tier
// substrate: `.call(this, a, b)` rewrites to direct `f(a, b)` (this
// dropped — class-method this is post-P4); `.apply(this, [a, b])`
// inlines the array-literal arg into a direct `f(a, b)`; `.bind
// (this, p)` synthesizes a wrapper FnDecl `__bound_<f>_<id>(env,
// rem...)` + factory FnDecl `__bind_create_<f>_<id>(partials...)`
// that returns a Closure capturing partials.
//
// Acceptance:
//   - .call / .apply / .bind all match bun output
//   - Multiple-partials bind, capturing local-var values
//   - Bool / Number / String return types

// 1. .call forwards args, drops thisArg
function greet(prefix: string, name: string): string {
  return prefix + name;
}
console.log(greet.call(null, "Hi, ", "Alice"));  // Hi, Alice
console.log(greet.call(null, "Hey, ", "Bob"));   // Hey, Bob

// 2. .apply with array-literal arg
console.log(greet.apply(null, ["Hello, ", "Carol"]));  // Hello, Carol

// 3. .bind with single partial → 1-remaining closure
const sayHi = greet.bind(null, "Hi, ");
console.log(sayHi("Dave"));   // Hi, Dave
console.log(sayHi("Eve"));    // Hi, Eve

// 4. .bind with multiple partials → 0/1-remaining closure
function add(a: number, b: number, c: number): number {
  return a + b + c;
}
const add10_20 = add.bind(null, 10, 20);
console.log(add10_20(5));   // 35
console.log(add10_20(100)); // 130

// 5. .call with 3 args
console.log(add.call(null, 1, 2, 3));  // 6

// 6. .apply with 3-element array literal
console.log(add.apply(null, [4, 5, 6]));  // 15

// 7. .bind captures local-var values (not just literals)
const x = 100;
const y = 200;
const add_xy = add.bind(null, x, y);
console.log(add_xy(50));    // 350
console.log(add_xy(-100));  // 200

// 8. Bool-returning fn
function gt(a: number, b: number): boolean {
  return a > b;
}
const gt5 = gt.bind(null, 5);
console.log(gt5(3));   // true (5 > 3)
console.log(gt5(10));  // false (5 > 10)
console.log(gt.call(null, 10, 5));  // true

// 9. Mixed: bind chain on the same source
const greetHi = greet.bind(null, "Hi, ");
const greetHey = greet.bind(null, "Hey, ");
console.log(greetHi("Frank"));   // Hi, Frank
console.log(greetHey("Grace"));  // Hey, Grace
