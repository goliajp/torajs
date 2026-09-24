// §10.4.4: an arguments object is ordinary past its argument list —
// `arguments[2] = v` on a two-argument call creates own property "2"
// and leaves `length` at 2, so the generic array algorithms that walk
// 0..length never see it.
function f(a: any, b: any) {
  arguments[3] = "x";
  arguments[2] = 9;
  const g: any = arguments;
  console.log(g.length, g[2], g[3], 2 in g, g.hasOwnProperty("3"));
  console.log(Object.keys(g).join(), JSON.stringify(Object.getOwnPropertyDescriptor(g, "2")));
  for (const k in g) console.log("in", k);
  console.log(Array.prototype.indexOf.call(g, 9), Array.prototype.slice.call(g).length);
  console.log(Array.prototype.every.call(g, (v: any) => v > 10));
  console.log(delete g[2], 2 in g, g[2], Object.keys(g).join(), delete g[7]);
  Array.prototype.push.call(g, 7);
  console.log(g.length, g[2], g[3], Object.keys(g).join());
}
f(12, 11);
function h() {
  arguments[1] = 5;
  return arguments[1] + ":" + arguments.length;
}
console.log(h(), h(1), h(1, 2));
