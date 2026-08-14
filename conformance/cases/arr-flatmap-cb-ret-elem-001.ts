function runA() {
  return [1].flatMap(function (x: number) { return "a" + x; })[0];
}
console.log(runA());
function runB() {
  return [1].flatMap((x: number) => [x + 1, x + 2])[1];
}
console.log(runB());
function runC() {
  return [5].flatMap((x: number) => x * 2)[0];
}
console.log(runC());
