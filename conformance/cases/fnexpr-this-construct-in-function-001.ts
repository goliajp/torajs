// A function expression or declaration nested in a plain or async
// function is constructible like a top-level one: `new` hands it the
// fresh instance (§10.2.2), a plain call undefined, `.call(o)` `o`.
// The async body here never awaits, so the nested functions stay
// inside it instead of being hoisted out.
function outer(): void {
  const F = function (this: any, a: number) { this.tag = a; };
  function G(this: any) { this.tag = 4; }
  const f = function (this: any) { return this === undefined; };
  function g(this: any) { return this === undefined; }
  const h = function (this: any) { return this.w; };
  function k(this: any) { return this.w; }
  const o: any = new (F as any)(3);
  console.log("sync new", o.tag, new (G as any)().tag, o instanceof F);
  console.log("sync call", f(), g());
  console.log("sync .call", h.call({ w: 1 }), k.call({ w: 2 }));
}
async function outerAsync(): Promise<void> {
  const F = function (this: any) { this.tag = 5; };
  function G(this: any) { this.tag = 15; }
  const f = function (this: any) { return this === undefined; };
  const h = function (this: any) { return this.w; };
  console.log("async new", new (F as any)().tag, new (G as any)().tag);
  console.log("async call", f(), h.call({ w: 3 }));
}
outer();
outerAsync().then(() => console.log("done"));
