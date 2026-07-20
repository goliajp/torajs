// A mixed-element array literal must not type its HOF callback param
// from the first element: `[1, Symbol()].forEach(props => ...)`
// inferred "number" and the Any elem coerce threw ToNumber(Symbol)
// at the param use site (rotation 162; test262 staging
// object-create-with-primitive-second-arg appeared case shape).
[1, "", true, Symbol(), undefined].forEach(props => {
  console.log(Object.getPrototypeOf(Object.create(null, props)) === null);
});
console.log(
  [1, "x", true]
    .map(v => typeof v)
    .join(",")
);
// homogeneous literals keep the typed fast path
console.log([3, 1, 2].map(n => n * 2).join(","));
