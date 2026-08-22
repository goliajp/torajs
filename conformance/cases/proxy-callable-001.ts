// §10.5.12 [[Call]] / §10.5.13 [[Construct]] — and what `typeof` says.
function base(a: number, b: number) { return a + b; }

const p: any = new Proxy(base as any, {
  apply(target: any, thisArg: any, args: any) {
    return "apply:" + args.length + ":" + args[0] + "," + args[1];
  },
});
console.log(typeof p);
console.log(p(1, 2));
console.log(p.call(null, 3, 4));
console.log(p.apply(null, [5, 6]));

// A trap-less proxy over a function forwards the call.
const q: any = new Proxy(base as any, {});
console.log(typeof q, q(7, 8));

// A proxy over a plain object is not callable.
const o: any = new Proxy({}, {});
console.log(typeof o);
try { o(); } catch (e: any) { console.log("not callable:", e instanceof TypeError); }

// construct
class Point { x: number; constructor(x: number) { this.x = x; } }
const c: any = new Proxy(Point as any, {
  construct(target: any, args: any, nt: any) {
    return { x: args[0] * 100, viaTrap: true };
  },
});
const made: any = new c(3);
console.log(made.x, made.viaTrap);

// A trap-less proxy over a class constructs the real thing.
const c2: any = new Proxy(Point as any, {});
const real: any = new c2(9);
console.log(real.x, real instanceof Point);

// The construct trap must answer an object.
const bad: any = new Proxy(Point as any, { construct() { return 1 as any; } });
try { new bad(1); } catch (e: any) { console.log("construct bad:", e instanceof TypeError); }

// A proxy over a non-constructor is not one.
const notCtor: any = new Proxy((() => 1) as any, {});
console.log(typeof notCtor);

// Proxy over proxy over a function stays callable.
const deep: any = new Proxy(new Proxy(base as any, {}), {});
console.log(typeof deep, deep(10, 20));
