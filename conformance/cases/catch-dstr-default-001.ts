// §14.15 CatchParameter : BindingPattern — defaults, nesting, rest,
// elision, alias-to-pattern all through the shared PatShape machine.
try {
  throw { x: undefined, y: 2 };
} catch ({ x = 1, y = 0, z = "d" }) {
  console.log(x, y, z);
}
try {
  throw [undefined, 5];
} catch ([a = 10, b = 20, c = 30]) {
  console.log(a, b, c);
}
try {
  throw { p: { q: 7 } };
} catch ({ p: { q } }) {
  console.log(q);
}
try {
  throw [1, 2, 3, 4];
} catch ([first, , ...rest]) {
  console.log(first, rest.length, rest[0], rest[1]);
}
try {
  throw { a: 1, b: 2, c: 3 };
} catch ({ a, ...others }) {
  console.log(a, others.b, others.c);
}
try {
  throw [[8, 9]];
} catch ([[m, n] = []]) {
  console.log(m, n);
}
try {
  throw { v: undefined };
} catch ({ v: w = "def" }) {
  console.log(w);
}
try {
  throw [3];
} catch ([q = 1]) {
  q = q + 1;
  console.log(q);
}
