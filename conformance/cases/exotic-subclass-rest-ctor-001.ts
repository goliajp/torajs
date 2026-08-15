// Ctor-less exotic-builtin subclasses forward the true call-site
// argc through the synthesized rest-param default ctor: 0 args ride
// the mint default (or Promise's executor TypeError), 1+ the
// one-argument super kernel. Pre-fix these were the no-forward
// shape — `new N2(5)` silently valueOf'd +0, `new A2(3)` came up
// empty, and the argument-carrying literal at the call site minted
// a typed-flavor array a synthesized Arr<Any> reader mis-decoded.
class N2 extends Number {}
class S2 extends String {}
class B2 extends Boolean {}
class A2 extends Array {}
class R2 extends RegExp {}
console.log(new N2(5).valueOf(), new N2().valueOf(), new N2(undefined).valueOf());
console.log(new S2("hi").valueOf(), new S2().valueOf().length);
console.log(new B2(true).valueOf(), new B2().valueOf());
console.log(new A2(3).length, new A2().length);
console.log(new R2("x").test("aaa"), new R2("a+").test("aaa"));
class P2 extends Promise<number> {}
const p = new P2((resolve: any) => {
  resolve(42);
});
p.then((v: number) => {
  console.log("got", v);
});
try {
  const bad = new (P2 as any)();
  console.log("no-throw");
} catch (e) {
  console.log("threw");
}
class D2 extends Date {}
console.log(new D2(0).getTime());
console.log(new D2("2020-01-02T00:00:00Z").getUTCFullYear());
console.log(new D2().getTime() > 0);
const ae = new A2(1, 2, 3);
console.log(ae.length, ae[0], ae[2], Array.isArray(ae), ae instanceof A2);
const as = new A2("x", "y");
console.log(as.length, as[1]);
