// rotation 275 刀 6 — §13.3.3 PropertyName : NumericLiteral /
// StringLiteral in object binding patterns: `{ 0: v, length: z }`
// reads index slots of an array-as-object source (rename mandatory —
// a numeric key cannot shorthand-bind); a string-literal key names
// any field.

let length = "outer";
const [...{ 0: v, 1: w, 2: x, 3: y, length: z }] = [7, 8, 9];
console.log(v, w, x, y, z);
console.log(length);

const { 0: first, "a b": spaced } = { 0: 10, "a b": 20 } as any;
console.log(first, spaced);

const arr = [1, 2, 3];
const { 1: second, length: len } = arr as any;
console.log(second, len);

// numeric key with a default
const { 5: missing = 99 } = [1] as any;
console.log(missing);

// nested pattern behind a numeric key
const { 0: [inner] } = [[42]] as any;
console.log(inner);

// param position — both readers share the recipe
function f([...{ 0: pv, 1: pw, length: pz }]: any) {
  console.log(pv, pw, pz);
}
f([7, 8, 9]);
function g({ 0: ga, "x y": gb }: any) {
  console.log(ga, gb);
}
g({ 0: 1, "x y": 2 });
