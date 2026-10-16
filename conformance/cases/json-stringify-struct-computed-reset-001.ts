// §25.5.2.4 SerializeJSONObject fixes K before it reads a value: a
// computed field declared before a layout member, deleted and set
// again mid-walk, is in K once and serializes once, in its K place.
const k = String(process.argv.length > 100 ? "q" : "kk");
class H {
  [k] = 1;
  b = 2;
  c = 3;
}
const h: any = new H();
console.log(JSON.stringify(h, function (key, v) {
  if (key === "b") {
    delete h[k];
    h[k] = 5;
  }
  return v;
}));
// the same when the replacer resets the field while it is the one read
const h2: any = new H();
console.log(JSON.stringify(h2, function (key, v) {
  if (key === k) {
    delete h2[k];
    h2[k] = [6];
  }
  return v;
}));
// and when a later member's toJSON does it
class I {
  [k] = 1;
  b = 2;
  c: any = 3;
}
const i: any = new I();
i.c = {
  toJSON() {
    delete i[k];
    i[k] = 7;
    return 4;
  },
};
console.log(JSON.stringify(i));
