// a chain of bound functions is constructed, tested with instanceof
// and released in one loop per chain: its depth never reaches the
// native stack. No line printed depends on N; bun overflows its own
// stack on `new` well before 50000 layers, so its output is taken at
// a depth it reaches.

const N = 50000;

function Pt(this: any, a: any, b: any, c: any) {
  this.xs = [a, b, c].join(",");
  this.nt = new.target;
}

// new over a deep chain: partials innermost first, and a newTarget
// equal to a layer is swapped for that layer's target, down to Pt
let b: any = Pt.bind(null, 1);
for (let i = 0; i < N; i++) b = b.bind(null);
let mid: any = b;
b = b.bind(null, 2);
for (let i = 0; i < N; i++) b = b.bind(null);
const o: any = new b(3);
console.log(o.xs, o.nt === Pt, o instanceof Pt, o instanceof b, o instanceof mid);
console.log(Object.getPrototypeOf(o) === Pt.prototype, ({}) instanceof b);

// a newTarget that is no layer of the chain is passed through
function Other() {}
const r: any = Reflect.construct(b, [4], Other);
console.log(r.xs, r.nt === Other, Object.getPrototypeOf(r) === Other.prototype);
// a newTarget that is a middle layer is swapped from there down
const r2: any = Reflect.construct(b, [5], mid);
console.log(r2.xs, r2.nt === Pt);

// a class at the bottom of the chain
class C {
  xs: string;
  nt: any;
  constructor(...xs: number[]) {
    this.xs = xs.join(",");
    this.nt = new.target;
  }
}
let cb: any = C;
for (let i = 0; i < N; i++) cb = cb.bind(null, i < 2 ? i : 9).bind(null);
const c: any = new cb(7);
console.log(c.xs.split(",").length === N + 1, c.xs.slice(0, 5), c.nt === C, c instanceof C, c instanceof cb);

// a Proxy in the middle: the outer layers swap newTarget down to the
// proxy, and the proxy forwards it unchanged into the inner layers
let inner: any = Pt;
for (let i = 0; i < N; i++) inner = inner.bind(null);
const px: any = new Proxy(inner, {});
let outer: any = px;
for (let i = 0; i < N; i++) outer = outer.bind(null);
const p: any = new outer(6);
console.log(p.xs, p.nt === px, p.nt === Pt, p instanceof inner);
try {
  console.log(p instanceof outer);
} catch (e: any) {
  console.log("throws", e instanceof TypeError);
}

// a construct trap sees the swapped newTarget
let seen: any = null;
const tp: any = new Proxy(Pt, {
  construct(t: any, args: any[], nt: any) {
    seen = nt;
    return { xs: args.join("+") };
  },
});
let tb: any = tp;
for (let i = 0; i < N; i++) tb = tb.bind(null, 0).bind(null);
const q: any = new tb(8);
console.log(q.xs.split("+").length === N + 1, seen === tp);

// a middle layer with its own @@hasInstance is asked, once
let hb: any = Pt;
for (let i = 0; i < N; i++) hb = hb.bind(null);
let calls = 0;
Object.defineProperty(hb, Symbol.hasInstance, {
  value: (x: any) => {
    calls++;
    return x === 42;
  },
});
for (let i = 0; i < N; i++) hb = hb.bind(null);
console.log(42 instanceof hb, ({}) instanceof hb, calls);

// a chain over a function with no [[Construct]] is no constructor
let ab: any = (x: number) => x;
for (let i = 0; i < N; i++) ab = ab.bind(null);
try {
  new ab(1);
  console.log("constructed");
} catch (e: any) {
  console.log(e instanceof TypeError, ab(9));
}

// releasing the outer half of a chain leaves the inner half whole
let keep: any = function kk(x: number) {
  return x * 2;
};
for (let i = 0; i < N; i++) keep = keep.bind(null);
let drop: any = keep;
for (let i = 0; i < N; i++) drop = drop.bind(null, i);
drop = null;
console.log(keep(21));

// and releasing a whole deep chain
let all: any = function aa() {
  return 1;
};
for (let i = 0; i < N; i++) all = all.bind({ i });
console.log(all());
all = null;
b = null;
mid = null;
cb = null;
outer = null;
tb = null;
hb = null;
ab = null;
keep = null;
console.log("done");
