// §13.2.4.1 spread, §23.1.2.1 Array.from and §10.4.2's rest packing
// all read the iterator. The typed lanes read the backing store
// instead, which is right until something installs a hook — and then
// one value has two meanings: `[...src]` answered the elements while
// `[...(src as any)]` answered the hook.

const src = [1, 2, 3];
(src as any)[Symbol.iterator] = function () {
  let i = 0;
  return {
    next: function () {
      return i < 2 ? { value: 10 + i++, done: false } : { value: undefined, done: true };
    },
  };
};

console.log(JSON.stringify([...src]));
console.log(JSON.stringify([0, ...src, 9]));
console.log(JSON.stringify(Array.from(src)));
console.log(JSON.stringify(Array.from(src, function (x: any) { return x * 2; })));

function take(...xs: any[]): number {
  return xs.length;
}
console.log(take(...src));
function three(a: any, b: any, c: any): string {
  return "" + a + "/" + b + "/" + c;
}
console.log(three(...src));

for (const x of src) {
  console.log("for-of", x);
}
const [p, q] = src;
console.log("dstr", p, q);

// a Set with its own hook takes the same road
const s = new Set<number>([1, 2, 3]);
(s as any)[Symbol.iterator] = function () {
  let i = 0;
  return { next: function () { return i < 1 ? { value: 42 + i++, done: false } : { value: undefined, done: true }; } };
};
console.log(JSON.stringify([...s]), JSON.stringify(Array.from(s)));

// the plain read of the intrinsic hook is untouched
const t = [7, 8];
console.log(JSON.stringify([...t]), typeof (t as any)[Symbol.iterator]);
