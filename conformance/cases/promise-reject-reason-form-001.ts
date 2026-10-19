// A rejected promise awaited at the top level throws its reason as
// the value it is (ECMA-262 §27.7.5.3 step 2), whatever form its cell
// holds it in: a fraction, an integer, a boolean.

try {
  const p: Promise<number> = Promise.reject(7.5);
  await p;
} catch (e) {
  console.log("fraction", e, typeof e);
}

try {
  const q: Promise<number> = Promise.reject(8);
  await q;
} catch (e) {
  console.log("integer", e, typeof e);
}

try {
  const b: Promise<boolean> = Promise.reject(true);
  await b;
} catch (e) {
  console.log("boolean", e, typeof e);
}
