// §25.5.2.4 step 6 takes the key list before step 8 reads a value: a
// getter that deletes a later member leaves its key in the walk, the
// replacer is called for it with undefined, and what the replacer
// answers is serialized.
const obj = {
  get a() {
    delete this.b;
    return 1;
  },
  b: 2,
  c: 3,
};
const seen: string[] = [];
const out = JSON.stringify(obj, function (key: string, value: any) {
  seen.push(key + ":" + String(value));
  if (key === "b") return "<replaced>";
  return value;
});
console.log(out);
console.log(seen.join(" "));

// Without a replacer the deleted member reads undefined and is omitted.
const plain = {
  get a() {
    delete this.b;
    return 1;
  },
  b: 2,
  c: 3,
};
console.log(JSON.stringify(plain));

// A member deleted before the call is not in the key list at all, so
// the replacer never sees it.
const before = { a: 1, b: 2, c: 3 };
delete before.b;
const seen2: string[] = [];
console.log(
  JSON.stringify(before, function (key: string, value: any) {
    seen2.push(key);
    return value;
  }),
);
console.log(seen2.join(" "));
