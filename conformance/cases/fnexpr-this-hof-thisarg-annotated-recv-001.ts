// §23.1.3's array iteration methods thread the call site's thisArg
// into the callback. Which receivers admit one was decided by the
// receiver binding's ANNOTATION: the census took `const a = [5]` and
// dropped `const a: number[] = [5]`, so writing the type down turned a
// working program into an unclaimed-receiver refusal. An annotation
// narrows what a binding holds; it cannot make it hold something else.
const r: any = { z: 2 };

function tag(v: any): string {
  const th = this as any;
  return String(v) + (th === undefined ? "u" : (th.z === 2 ? "R" : "o"));
}

const F = function (v: any): any {
  const th = this as any;
  return String(v) + (th === undefined ? "u" : (th.z === 2 ? "R" : "o"));
};

// The two spellings that already worked.
console.log([5].map(F, r).join(","));
const plain = [5];
console.log(plain.map(F, r).join(","));

// The same program with the type written down.
const anys: any[] = [5];
console.log(anys.map(F, r).join(","));

const nums: number[] = [5];
console.log(nums.map(F, r).join(","));

const strs: string[] = ["q"];
console.log(strs.map(function (v: string): string {
  const th = this as any;
  return v + (th === undefined ? "u" : (th.z === 2 ? "R" : "o"));
}, r).join(","));

// The rest of the family reads the same census.
anys.forEach(function (v: any): void {
  const th = this as any;
  console.log(String(v) + (th === undefined ? "u" : (th.z === 2 ? "R" : "o")));
}, r);

console.log(anys.filter(function (v: any): boolean {
  const th = this as any;
  return th.z === 2;
}, r).join(","));

console.log(anys.some(function (v: any): boolean {
  const th = this as any;
  return th.z === 2;
}, r));

// With no thisArg the answer is `undefined`, annotated or not — the
// arm that was already right, kept here so a regression that stops
// threading shows up as a diff rather than as this file agreeing by
// accident.
console.log(anys.map(F).join(","));
console.log(plain.map(F).join(","));

// `tag` is a declaration rather than an expression; it reaches the
// same slot through the named-fn mirror.
console.log(nums.map(tag, r).join(","));
