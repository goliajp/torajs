// §20.1.2.5 EnumerableOwnProperties takes the keys first, then asks each
// key's [[GetOwnProperty]] as it reaches it: a getter that hides or
// deletes a later member leaves that member out of entries and values.
const e1 = Object.entries({
  get a() {
    Object.defineProperty(this, "b", { enumerable: false });
    return "A";
  },
  b: "B",
});
console.log(JSON.stringify(e1));
const v1 = Object.values({
  get a() {
    Object.defineProperty(this, "b", { enumerable: false });
    return "A";
  },
  b: "B",
});
console.log(JSON.stringify(v1));
const e2 = Object.entries({
  get a() {
    delete this.b;
    return 1;
  },
  b: 2,
  c: 3,
});
console.log(JSON.stringify(e2));
const v2 = Object.values({
  get a() {
    delete this.b;
    return 1;
  },
  b: 2,
  c: 3,
});
console.log(JSON.stringify(v2));
let x: any = { get a() { Object.defineProperty(this, "b", { enumerable: false }); return "A"; }, b: "B" };
console.log(JSON.stringify(Object.entries(x)));
x = { get a() { Object.defineProperty(this, "b", { enumerable: false }); return "A"; }, b: "B" };
console.log(JSON.stringify(Object.values(x)));
