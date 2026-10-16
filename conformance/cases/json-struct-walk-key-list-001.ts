// §25.5.2.4 takes the key list K before step 8 reads a value, and
// each member of K is read with a full [[Get]].

// A getter hides a later member: it is still in K, so it is read.
const hidden = {
  get x() {
    Object.defineProperty(this, "y", { enumerable: false });
    return 1;
  },
  y: 2,
  z: 3,
};
console.log(JSON.stringify(hidden));
console.log(JSON.stringify(hidden));

// The replacer deletes a later accessor member: it stays in K, its
// getter is gone, so it reads undefined and is omitted.
const acc = {
  get m() {
    return 1;
  },
  get n() {
    return 2;
  },
  o: 3,
};
const seen: string[] = [];
console.log(
  JSON.stringify(acc, function (this: any, k: string, v: any) {
    seen.push(k + ":" + String(v));
    if (k === "m") delete this.n;
    return v;
  }),
);
console.log(seen.join(" "));

// A getter on an any receiver deletes a later accessor member.
const viaAny: any = {
  get m() {
    delete this.n;
    return 1;
  },
  get n() {
    return 2;
  },
  o: 3,
};
console.log(JSON.stringify(viaAny));

// A data member redefined as an accessor answers its getter.
const redefined = { u: 1, v: 2 };
Object.defineProperty(redefined, "u", {
  get() {
    return 10;
  },
  enumerable: true,
});
console.log(JSON.stringify(redefined));
