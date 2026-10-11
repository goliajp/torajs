// a parameter default cannot see the body's declarations, so a body
// binding with the same name does not hide the outer one's dead zone
function f(y = K) {
  let K = 2;
  return y + K;
}
const g = (y = K) => {
  const K = 3;
  return y + K;
};
try {
  f();
} catch (e) {
  console.log("f early:", e instanceof ReferenceError);
}
try {
  g();
} catch (e) {
  console.log("g early:", e instanceof ReferenceError);
}
const K = 1;
console.log(f(), g(), f(10), g(10));
