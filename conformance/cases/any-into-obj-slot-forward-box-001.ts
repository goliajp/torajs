// a binding a closure declared above it captures is filled into a box
// the closure already shares; an any value written into that binding
// is cast against the binding's annotation all the same
type Pt = { x: number; y: number };
function f(s: string, t: string): string {
  const show = () => xs.length + ":" + xs.map((p) => p.x + p.y).join(",");
  const xs: Pt[] = JSON.parse(s);
  xs.push(JSON.parse(t));
  return show();
}
console.log(f('[{"x":1,"y":2}]', '{"x":3,"y":4.5}'));
function g(s: string): number {
  const read = () => p.x;
  let p: Pt = JSON.parse(s);
  p = JSON.parse('{"x":7,"y":8}');
  return read() + p.y;
}
console.log(g('{"x":1,"y":2}'));
