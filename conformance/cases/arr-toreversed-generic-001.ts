// Array.prototype.toReversed on a generic array-like receiver —
// §23.1.3.33 descending Get walk, dense product (test262
// toReversed/get-descending-order shape; any-annotated literal =
// dynobj lane — the unannotated struct lane drops numeric-key
// getters, a recorded pre-existing gap).
let order: number[] = [];
let arrayLike: any = {
  length: 3,
  get 0() { order.push(0); return "x0"; },
  get 1() { order.push(1); return "x1"; },
  get 2() { order.push(2); return "x2"; },
};
let r0 = Array.prototype.toReversed.call(arrayLike);
console.log(order.join(","), r0.join("|"));
// holes read as undefined, product is dense
let al2: any = { length: 4, 0: "a", 2: "c" };
let r1 = Array.prototype.toReversed.call(al2);
console.log(r1.length, JSON.stringify(r1));
// primitive receiver: ToObject owns no length, empty product
let rb = Array.prototype.toReversed.call(true as any);
console.log(Array.isArray(rb), rb.length);
// real-array lane unchanged
console.log([1, 2, 3].toReversed().join(","));
