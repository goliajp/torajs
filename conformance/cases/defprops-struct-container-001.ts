// struct-literal props container for defineProperties (TAG_OBJ arm:
// layout walk + two-phase validate/apply, keys minted + released)
const o: any = {};
Object.defineProperties(o, {
  a: { value: 1, enumerable: true },
  b: { value: "two", enumerable: true, writable: true },
});
console.log(o.a, o.b);
console.log(Object.keys(o).length);

// Object.create with a struct props container
const p: any = Object.create(null, { x: { value: 42, enumerable: true } });
console.log(p.x);

// a non-object descriptor member rejects (§20.1.2.3.1 step 5.b)
const badProps: any = { bad: 5 };
const tgt: any = {};
let caught = "";
try {
  Object.defineProperties(tgt, badProps);
} catch (e: any) {
  caught = e.name;
}
console.log(caught);

// absent flags default false — key exists but is not enumerable
const q: any = {};
Object.defineProperties(q, { k: { value: 9 } });
console.log(q.k, Object.keys(q).length);
