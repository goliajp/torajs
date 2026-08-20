// rotation 460 — `<anything>.prototype.k = function () { … this … }`.
// The store arm used to require the prototype's owner be a bare name
// or an inline function, which refused the member-chain spelling
// (`ns.Ctor.prototype.m = …`) even though how the prototype object was
// reached changes nothing about the channel: an instance method call
// resolves the name up the chain in the any lane, which shifts argv on
// FLAG_CLOSURE_RECV_FIRST.
var ns: any = {
  Ctor: function () {
    (this as any).tag = 41;
  },
};
ns.Ctor.prototype.bump = function () {
  return (this as any).tag + 1;
};
ns.Ctor.prototype.echo = function (p: any, q: any) {
  return [typeof this, p, q].join(",");
};
var inst = new ns.Ctor();
console.log(inst.bump());
console.log(inst.echo(1, 2));
var detached: any = inst.echo;
console.log(detached(3, 4));
