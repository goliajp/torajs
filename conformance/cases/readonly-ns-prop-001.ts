// §21.1.2 / §20.2.2 / §21.3.1 — strict write to a non-writable
// builtin-namespace property raises TypeError at the site.
let caught = 0;
try {
  Number.MAX_VALUE = 42;
} catch (e) {
  caught++;
  console.log(e instanceof TypeError);
}
try {
  Math.PI = 3;
} catch (e) {
  caught++;
}
console.log(caught, Number.MAX_VALUE > 1e308, Math.PI);
