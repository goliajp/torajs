// property names that look like keywords or carry escapes, and
// `async` followed by a line break: all ordinary data properties
const o = { get: 5, set: 6, get: 1, set: 2, async: 3 };
console.log(o.get, o.set, o.async);
const a = {
  async
  : 7,
};
console.log(a.async);
const b = { async() { return 1; }, get g() { return 2; }, async m() { return 3; } };
console.log(typeof b.async, b.g, typeof b.m);
const c = { "k": 1, 2: "two", ["c" + 1]: 3 };
console.log(c.k, c[2], c.c1);
