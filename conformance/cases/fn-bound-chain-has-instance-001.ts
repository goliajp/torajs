// instanceof over a deep chain of bound functions where a middle layer
// carries its own @@hasInstance: a getter, a Proxy over a function, a
// class, a handler that throws, a getter that throws, and a value that
// is not callable. Each is asked as the top-level operator asks it
// (§13.10.2 steps 2-3, §7.3.11), with the layer as `this`. Then
// Reflect.construct over a Proxy with a newTarget, under bound layers.
// No line printed depends on N; bun overflows its own stack well before
// 50000 layers, so its output is taken at a depth it reaches.
const N = 50000;

function Pt(this: any, a: any) {
  this.a = a;
  this.nt = new.target;
}

function chain(base: any, n: number): any {
  let b = base;
  for (let i = 0; i < n; i++) b = b.bind(null);
  return b;
}

function withHandler(desc: any): any {
  const mid = chain(Pt, N);
  Object.defineProperty(mid, Symbol.hasInstance, desc);
  return { mid, top: chain(mid, N) };
}

function test(label: string, top: any, x: any) {
  try {
    console.log(label, x instanceof top);
  } catch (e: any) {
    console.log(label, "throws", e instanceof TypeError ? "TypeError" : String(e));
  }
}

// a getter: called once per test, with the layer as `this`
let gets = 0;
let getThis: any = null;
function getter(this: any) {
  gets++;
  getThis = this;
  return (x: any) => x === 7;
}
const g = withHandler({ get: getter });
test("getter", g.top, 7);
test("getter", g.top, {});
console.log("getter calls", gets, getThis === g.mid);

// a Proxy over a function as the handler
let seenThis: any = null;
function isStr(this: any, x: any) {
  seenThis = this;
  return typeof x === "string";
}
const ph = new Proxy(isStr, {});
const p = withHandler({ value: ph });
test("proxy", p.top, "s");
test("proxy", p.top, 1);
console.log("proxy this", seenThis === p.mid);

// a proxy with an apply trap
let trapped = 0;
const pt = withHandler({
  value: new Proxy(() => false, {
    apply(_t: any, thisArg: any, args: any[]) {
      trapped++;
      return thisArg === pt.mid && args[0] === 3;
    },
  }),
});
test("apply trap", pt.top, 3);
test("apply trap", pt.top, 4);
console.log("apply calls", trapped);

// a class as the handler: calling it throws
class H {}
const c = withHandler({ value: H });
test("class", c.top, {});

// a handler that throws
const t = withHandler({
  value() {
    throw new RangeError("no");
  },
});
try {
  console.log(({}) instanceof t.top);
} catch (e: any) {
  console.log("throwing handler", e instanceof RangeError, e.message);
}

// a getter that throws
const tg = withHandler({
  get() {
    throw new SyntaxError("get");
  },
});
try {
  console.log(({}) instanceof tg.top);
} catch (e: any) {
  console.log("throwing getter", e instanceof SyntaxError, e.message);
}

// handlers that are not callable: a number and a plain object
test("number", withHandler({ value: 1 }).top, {});
test("object", withHandler({ value: {} }).top, {});
// null and undefined are no handler: the walk goes on to Pt
const nu = withHandler({ value: null });
test("null", nu.top, new Pt(1));
test("undefined", withHandler({ value: undefined }).top, new Pt(1));
test("null miss", nu.top, {});
// the result is coerced
test("coerced", withHandler({ value: () => "yes" }).top, 0);
test("coerced", withHandler({ value: () => 0 }).top, new Pt(1));

// Reflect.construct over a proxy with a newTarget, under bound layers
function NT() {}
let ntSeen: any = null;
const cp: any = new Proxy(Pt, {
  construct(target: any, args: any[], nt: any) {
    ntSeen = nt;
    return Reflect.construct(target, args, nt);
  },
});
const r: any = Reflect.construct(cp, [1], NT);
console.log("reflect", r.a, r.nt === NT, ntSeen === NT, Object.getPrototypeOf(r) === NT.prototype);
const bcp = chain(cp, N);
const r2: any = Reflect.construct(bcp, [2], NT);
console.log("bound reflect", r2.a, r2.nt === NT, ntSeen === NT, r2 instanceof NT);
const r3: any = Reflect.construct(bcp, [3], bcp);
console.log("bound self", r3.a, r3.nt === cp, ntSeen === cp, r3 instanceof Pt);
const r4: any = new bcp(4);
console.log("new bound", r4.a, r4.nt === cp, ntSeen === cp);
// a proxy without a construct trap
const bare: any = chain(new Proxy(Pt, {}), N);
const r5: any = Reflect.construct(bare, [5], NT);
console.log("bare", r5.a, r5.nt === NT, Object.getPrototypeOf(r5) === NT.prototype);
console.log("done");
