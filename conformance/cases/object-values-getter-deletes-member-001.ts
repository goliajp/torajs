// §20.1.2.22 EnumerableOwnProperties takes the key list first and asks
// each key's [[GetOwnProperty]] when it reaches it: a getter that
// deletes a later member leaves it out, whatever the member's type,
// an accessor member included. §25.5.2.4 SerializeJSONObject reads
// the same way.
const e1 = Object.entries({
  get a() {
    delete this.b;
    return 1;
  },
  b: [1],
  c: 2,
});
console.log(JSON.stringify(e1));
const v1 = Object.values({
  get a() {
    delete this.b;
    return 1;
  },
  b: [1],
  c: 2,
});
console.log(JSON.stringify(v1));
const v2 = Object.values({
  get a() {
    delete this.g;
    return 1;
  },
  get g() {
    return 5;
  },
  c: 2,
});
console.log(JSON.stringify(v2));
const e2 = Object.entries({
  get a() {
    delete this.g;
    return 1;
  },
  get g() {
    return 5;
  },
  c: 2,
});
console.log(JSON.stringify(e2));
const o3 = {
  get a() {
    delete (o3 as any).b;
    return "x";
  },
  b: { k: 1 },
  c: "s",
};
console.log(JSON.stringify(Object.entries(o3)));
const o4 = {
  get a() {
    delete (this as any).g;
    return 1;
  },
  get g() {
    return "G";
  },
  c: "c",
};
console.log(JSON.stringify(Object.values(o4)), JSON.stringify(Object.keys(o4)));
const o = {
  get a() {
    delete (this as any).g;
    return 1;
  },
  get g() {
    return 5;
  },
  c: 2,
};
console.log(JSON.stringify(o));
const p = {
  get a() {
    delete (this as any).b;
    return 1;
  },
  b: [1],
  c: "c",
};
console.log(JSON.stringify(p));
