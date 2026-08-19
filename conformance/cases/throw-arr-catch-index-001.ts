// A throw is a boxing boundary: a typed array crossing into the catch
// binding's any must be kind-marked, or the kind-aware index readers
// answer undefined for every slot (`e.length` was fine, `e[0]` was not).
try {
  throw [7, 8];
} catch (e) {
  console.log(e[0], e[1], e.length);
}
try {
  throw [1.5, 2.5];
} catch (e) {
  console.log(e[0] + e[1]);
}
try {
  throw [[1, 2], [3]];
} catch (e) {
  console.log(e[0][1], e[1][0]);
}
try {
  throw ["a", "b"];
} catch (e) {
  console.log(e[0], e[1]);
}
try {
  throw [true, false];
} catch (e) {
  console.log(e[0], e[1]);
}
