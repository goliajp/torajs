// 399-02 — a fn-expr in an array-literal ELEMENT position, when the
// literal initializes an `: any` binding, gets its call-site `this`:
// the annotation makes every element read a box, so each call path
// rides the any lane (which shifts argv on FLAG_CLOSURE_RECV_FIRST).

// .call seeds the given receiver
const arr: any = [function (n: any) { (this as any).n = n }];
const bag: any = {};
arr[0].call(bag, 5);
console.log(bag.n);

// index-call: §13.3.6.2 — the receiver is the array itself
const a6: any = [function () { return (this as any)[1] }, 77];
console.log(a6[0]());

// .apply
const a5: any = [function (m: any) { return (this as any).v + m }];
console.log(a5[0].apply({ v: 40 }, [2]));

// this-free element is untouched by the promotion
const a3: any = [function (x: any) { return x + 1 }];
console.log(a3[0](41));

// an unannotated (typed) binding stays out of the face
const t4 = [function (x: number) { return x * 2 }];
console.log(t4[0](21));
