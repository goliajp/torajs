// an `any` value written into an annotated binding is cast to that
// binding's own annotation: one whose initializer is a JSON.parse call,
// one an inner block's same-named binding shadowed for a while, one a
// closure or a generator captures, one a loop makes per iteration, one
// the body of a top-level loop declares

type P = { x: number; y: string };
type H = { b: { items: P[] } };

// annotated bindings a JSON.parse call initializes, then appended to
// (one value or two) and assigned
function parsed(s: string, t: string): void {
  const cfg: H = JSON.parse(s);
  cfg.b.items.push(JSON.parse(t));
  console.log("cfg", cfg.b.items.length, cfg.b.items[1].x, cfg.b.items[1].y);
  const xs: P[] = JSON.parse("[" + t + "]");
  xs.push(JSON.parse(t), JSON.parse('{"x":3.5,"y":"c"}'));
  console.log("xs", xs.length, xs[1].x, xs[2].x, xs[2].y);
  let p: P = JSON.parse(t);
  p = JSON.parse('{"x":4.25,"y":"e"}');
  console.log("p", p.x, p.y);
}
parsed('{"b":{"items":[{"x":1,"y":"a"}]}}', '{"x":2.5,"y":"b"}');

// the outer `xs` again once the block that shadowed it closes
function shadowed(t: string): void {
  const xs: P[] = [];
  let q: P = { x: 0, y: "z" };
  {
    const xs = [1, 2];
    let q = 3;
    q = q + xs.length;
    console.log("inner", xs.length, q);
  }
  xs.push(JSON.parse(t));
  q = JSON.parse(t);
  console.log("outer", xs.length, xs[0].x, q.x, q.y);
}
shadowed('{"x":5.5,"y":"f"}');

// a generator body writes the binding it captures
function generated(t: string): void {
  let q: P = { x: 0, y: "z" };
  const g = function* () {
    q = JSON.parse(t);
    yield q.x;
  };
  for (const n of g()) console.log("yield", n);
  console.log("gen", q.x, q.y);
}
generated('{"x":6.5,"y":"g"}');

// closures over a per-iteration binding and over a loop-head binding
function looped(t: string): void {
  const fs: (() => void)[] = [];
  for (let i = 0; i < 2; i++) {
    let ys: P[] = [];
    fs.push(() => {
      ys.push(JSON.parse(t));
      console.log("iter", i, ys.length, ys[0].x);
    });
  }
  for (const f of fs) f();
  for (let zs: P[] = []; zs.length < 2; ) {
    const add = (): number => zs.push(JSON.parse(t));
    add();
    console.log("head", zs.length, zs[0].y);
  }
}
looped('{"x":7.5,"y":"h"}');

// a binding the body of a top-level loop declares, of a type no other
// write reaches
type W = { v: number; w: string };
for (let i = 0; i < 2; i++) {
  let ws: W[] = [];
  ws.push(JSON.parse('{"v":8.5,"w":"k"}'), JSON.parse('{"v":9,"w":"l"}'));
  console.log("top", i, ws.length, ws[0].v, ws[1].w);
}
