// A fractional argument reaching an object-literal method or setter that
// reads `this` keeps its fraction: the receiver is not an argument
// position, so the value lines up with the declared parameter.

const m = { v: 1, put(x: number) { this.v = x + 1; } };
m.put(4);
console.log("m int", m.v);
m.put(4.5);
console.log("m float", m.v);

const n = { v: 1, set val(x: number) { this.v = x + 1; } };
n.val = 4;
console.log("n int", n.v);
n.val = 4.5;
console.log("n float", n.v);

const q = {
  v: 1,
  get val() { return this.v * 2; },
  set val(x: number) { this.v = x; },
};
q.val = 3;
q.val += 0.25;
console.log("q compound", q.v, q.val);
