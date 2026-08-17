// param-shadow-class (rotation 428 discovery) — the value-position
// class-reference rewrite must respect lexical scope: a local binding
// spelling a class name owns its references. The flat arena scan
// silently handed them to the class object (`helper(5)` answered
// `class R {}1` — the class's toString under `+`).
class R {
  static tag = "the-class";
}

// param shadows
function helper(R: any) {
  return R + 1;
}
console.log(helper(5));

// let shadows
function localLet() {
  let R = 10;
  return R + 1;
}
console.log(localLet());

// catch param shadows exactly the catch body
function viaCatch() {
  try {
    throw 7;
  } catch (R) {
    return (R as any) + 1;
  }
}
console.log(viaCatch());

// an unshadowed reference still resolves the class value
function real() {
  return R.tag;
}
console.log(real());

// arrow param shadows; enclosing fn still sees the class
const arrowShadow = (R: any) => R + 2;
console.log(arrowShadow(1), R.tag);
