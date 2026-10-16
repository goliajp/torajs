// §25.5.2.4 SerializeJSONObject over a class instance with many
// expando keys, and with keys the replacer deletes, sets again or adds
// while the walk runs: K is fixed at step 6, step 8 reads each key of K
// when it reaches it.
class Bag {}
const b: any = new Bag();
for (let i = 0; i < 5000; i++) b["k" + i] = i;
const s = JSON.stringify(b);
console.log(s.length, s.slice(0, 24), s.slice(-24));

const c: any = new Bag();
for (let i = 0; i < 6; i++) c["p" + i] = i;
const t = JSON.stringify(c, (k, v) => {
  if (k === "p1") {
    delete c.p5;
    c["p" + "5"] = 55;
    for (let j = 0; j < 40; j++) c["z" + j] = j;
  }
  if (k === "p2") delete c.p4;
  return v;
});
console.log(t);
console.log(Object.keys(c).length);
