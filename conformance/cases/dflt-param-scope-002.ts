// a nested function declaration that captures its scope becomes a value,
// and no call site of a value pads its defaults: they have to run in the
// function itself (ECMA-262 §10.2.11)
function literal(k: number) {
  function inner(y: number = 5) {
    return y + k;
  }
  return inner;
}
const f2 = literal(1);
console.log("literal default, capturing body:", f2(), f2(10), literal(2)());

function prior(k: number) {
  function inner(a: number, b: number = a * 2, c: string = "c") {
    return `${a + k}/${b}/${c}`;
  }
  return inner;
}
const f3 = prior(1);
console.log("prior param:", f3(1), f3(1, 5), f3(2, undefined, "z"));
