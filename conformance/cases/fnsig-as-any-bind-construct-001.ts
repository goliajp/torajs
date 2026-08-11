function pick(a: any, b: any) {
  return { sum: a + b };
}
var B = (pick as any).bind(null, 1);
var inst = new B(2);
console.log(inst.sum);
var T = pick as any;
console.log(typeof T);
var BT = T.bind(null, 4);
console.log(new BT(5).sum);
var C = function () {
  return new Boolean(true);
};
var BC = Function.prototype.bind.call(C);
console.log(new BC().valueOf());
