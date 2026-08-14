// A function expression whose body says `this` gets its receiver
// promoted only when every use of the binding is one the promoted
// ABI can survive. Naming it as a member's OBJECT is one of those:
// `K.s = …` never calls K, and `K.s()` calls what the property
// holds. Before this admitted, a single static-method store took the
// whole binding off the lane and the CONSTRUCTOR died on `__this`.
const K: any = function () {
  this.x = 1;
};
K.prototype.m = function (): number {
  return this.x + 1;
};
K.s = function (): number {
  return 40;
};
console.log(K.s(), new K().m());
