// f.apply / Reflect.apply with a NON-Array argArray — §7.3.18
// CreateListFromArrayLike admits any object read array-like (r292:
// the kernel used to refuse everything but a dense Arr cell).
function pair(a: any, b: any) {
  console.log(a, b);
}
const p: any = pair;

// array-like object
p.apply(null, { length: 2, 0: "a", 1: "b" });

// a FUNCTION as argArray — length is its arity, elements undefined
// (the 15.3.4.3-*-s family's shape)
function tf(x: number, y: number) {
  return x + y;
}
p.apply(null, tf);

// short / fractional / negative lengths
p.apply(null, { length: 1, 0: "only" });
p.apply(null, { length: -3 });
p.apply(null, { length: 1.7, 0: "trunc" });

// dense Arr fast lane regression
p.apply(null, [1, 2]);

// primitives stay TypeError
try {
  p.apply(null, 5);
} catch (e) {
  console.log("caught number");
}
try {
  p.apply(null, "ab");
} catch (e) {
  console.log("caught string");
}

// poisoned length getter forwards
try {
  p.apply(null, {
    get length(): number {
      throw new TypeError("poison");
    },
  });
} catch (e) {
  console.log("caught poison");
}

// Reflect.apply rides the same kernel
console.log(Reflect.apply(pair, null, { length: 2, 0: "r0", 1: "r1" }));
