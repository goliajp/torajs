// An index assignment's value, kept — with a balanced ledger.
//
// `c = (t[0] = [4, 5])` answered the right VALUE but underflowed the
// refcount at teardown: the slot's packing takes an owned temp's
// stake by transfer, so the returned operand was a borrow of a stake
// that had already moved into the bucket, and a consumer that kept
// it released a reference nobody owned. This is the face rotation
// 323's assignment-value knife explicitly left ("Index target lanes
// stay borrows"); it joins the same contract now — the consumer
// receives a fresh owned reference.

// any-receiver, numeric key, kept value
var t: any = [0, 0];
var c: any = 0;
c = (t[0] = [4, 5]);
console.log(String(c), String(t[0]));

// a borrow-shape rhs
var a = [4, 5, 6];
var c2: any = 0;
c2 = (t[1] = a);
console.log(String(c2), t[1].length);

// dynamic string key
var o: any = {};
var k = "kk";
var c3: any = 0;
c3 = (o[k] = [6, 7]);
console.log(String(c3), String(o.kk));

// typed tier stays put
var nums = [0, 0];
var c4 = 0;
c4 = (nums[0] = 9);
console.log(c4, nums[0]);

// Array<Any> element
var anyarr: any[] = [0, 0];
var c5: any = 0;
c5 = (anyarr[1] = "s");
console.log(String(c5), String(anyarr[1]));

// discard sites (statement position) — the loop must stay balanced
var churn: any = [0];
for (let i = 0; i < 100; i++) {
  churn[0] = [i, i];
}
console.log(String(churn[0]));

// the value is usable in an expression
var t2: any = [0];
var sum = (t2[0] = 20) + 5;
console.log(sum);
