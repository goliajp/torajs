function Pair(a: any, b: any) {
  (this as any).sum = a + b;
}
var B = (Pair as any).bind(null, 1);
var inst = new B(2);
console.log(inst.sum);
var direct = (Pair as any).bind({ sum: 0 });
var inst2 = new direct(5, 6);
console.log(inst2.sum);
function Tag(x: any) {
  (this as any).v = x;
}
var T = Tag as any;
console.log(typeof T);
var BT = T.bind(null, 9);
console.log(new BT().v);
var C = function () {
  return new Boolean(true);
};
var BC = Function.prototype.bind.call(C);
console.log(new BC().valueOf());
