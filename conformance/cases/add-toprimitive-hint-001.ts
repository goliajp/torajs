// §13.15.3 steps 1-2 — `+`'s object operands run ToPrimitive with
// the DEFAULT hint (valueOf before toString) BEFORE the
// string/number split; template substitutions keep the STRING hint
// (§13.2.8.5). Pre-fix every concat lane stringified objects with
// the string hint (toString first), and `${o}` could not diverge
// from `"" + o` because the desugar erased the hint.

const o: any = {
  valueOf() { console.log("valueOf"); return 1; },
  toString() { console.log("toString"); return "S"; },
};

// concat with a typed string: default hint
console.log("" + o); // valueOf -> 1

// any + any concat
const e: any = "";
console.log(e + o); // valueOf -> 1

// compound assignment concat
let s = "x";
s += o;
console.log(s); // valueOf -> x1

// number + object: both numeric after ToPrimitive
console.log(1 + o); // valueOf -> 2

// two valueOf-objects add NUMERICALLY (the raw-operand split
// concatenated their toStrings)
const p: any = { valueOf() { return 41; } };
const q: any = { valueOf() { return 1; } };
console.log(p + q); // 42

// template substitution keeps the string hint
console.log(`${o}`); // toString -> S
console.log(`v=${o}!`); // toString -> v=S!

// String() keeps the string hint
console.log(String(o)); // toString -> S

// valueOf answering an object falls through to toString
const r: any = { valueOf() { return this; }, toString() { return "R"; } };
console.log("" + r); // R

// plain objects and arrays keep their surfaces
console.log("" + [1, 2]); // 1,2
console.log("done");
