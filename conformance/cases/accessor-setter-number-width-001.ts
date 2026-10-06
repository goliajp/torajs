// A descriptor setter with a `number` parameter takes whichever number
// the assignment stored: an integer into an f64 slot and a fraction
// into an i64 slot both used to arrive as raw bits.
function b2(n: number) {
  const o: any = { n };
  let acc = 0.5;
  Object.defineProperty(o, "twice", { set(v: number) { acc = acc + v; }, configurable: true });
  o.twice = n + 1;
  console.log("b2", acc);
}
b2(3);
function b3(n: number) {
  const o: any = { n: 0.5 };
  Object.defineProperty(o, "twice", { set(v: number) { o.n = v; }, configurable: true });
  o.twice = n + 1;
  console.log("b3", o.n);
}
b3(3);
function b4(n: number) {
  const o: any = { n: n as any };
  Object.defineProperty(o, "twice", { get() { return o.n * 2; }, set(v: number) { o.n = v; }, configurable: true });
  o.twice = n + 1;
  console.log("b4", o.n, o.twice);
}
b4(3);
