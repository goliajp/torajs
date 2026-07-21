// §23.1.3.36 / S11.1.4 family — reading `xs.toString` as a VALUE on
// a typed array receiver answers the same reified method cell as
// `Array.prototype.toString` (RFC 20260721-array-proto-cluster 刀 3 /
// G5c; it used to read the expando side table only and answer
// undefined).
const arr = [1, 2, 3];
console.log(typeof arr.toString); // function
console.log(arr.toString === Array.prototype.toString); // true
console.log(arr.toString !== undefined); // true

const empty: any[] = [];
console.log(empty.toString === Array.prototype.toString); // true

const strs = ["a", "b"];
console.log(strs.toString === Array.prototype.toString); // true

// The call face keeps working (join(",") semantics).
console.log(arr.toString()); // 1,2,3

// toLocaleString rides the same face.
console.log(typeof arr.toLocaleString); // function
console.log(arr.toLocaleString === Array.prototype.toLocaleString); // true
