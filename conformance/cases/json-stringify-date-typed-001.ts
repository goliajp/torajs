// Typed-tier JSON.stringify over Date (rotation 185): direct call,
// struct field, typed array element; invalid date -> "null".
const d = new Date(1700000000000);
console.log(JSON.stringify(d));
const bad = new Date(NaN);
console.log(JSON.stringify(bad));
const obj = { when: d, n: 1 };
console.log(JSON.stringify(obj));
const arr = [d, bad];
console.log(JSON.stringify(arr));
const nested = { list: [d], meta: { t: bad } };
console.log(JSON.stringify(nested));
