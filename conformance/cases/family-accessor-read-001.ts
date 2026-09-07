// §24.1.3.10 / §24.2.3.9 — `size` is an accessor on Map.prototype /
// Set.prototype. Three faces of that table already asked
// `proto_tag_accessor_mid`: the descriptor synthesis,
// `getOwnPropertyNames`, and `in`. The member READ did not — `m.size`
// answers through its own intrinsic, so the gap only showed on the
// spellings that go through the general cascade.

const m: any = new Map<string, number>();
m.set("a", 1);
m.set("b", 2);
const s: any = new Set<number>();
s.add(1);

const k = "si" + "ze";
console.log(m.size, m["size"], m[k], Reflect.get(m, "size"), Reflect.get(m, k));
console.log(s.size, s["size"], s[k], Reflect.get(s, "size"));

// The faces that already agreed, still agreeing.
console.log("size" in m, Map.prototype.hasOwnProperty("size"), Object.hasOwn(m, "size"));
console.log(Object.getOwnPropertyDescriptor(Map.prototype, "size") !== undefined);

// It tracks the live count.
m.delete("a");
console.log(m[k], Reflect.get(m, "size"));

// Reading it off the prototype is a brand mismatch, not a value.
try {
  console.log((Map.prototype as any)[k]);
  console.log("no throw");
} catch (e) {
  console.log("proto read threw", e instanceof TypeError);
}

// An own expando shadows the family accessor.
const m2: any = new Map<string, number>();
m2.set("x", 1);
Object.defineProperty(m2, "size", { value: 99, configurable: true });
console.log(m2[k], m2.size);

// The methods around it, unchanged, and a miss is still undefined.
console.log(typeof m["g" + "et"], typeof s["a" + "dd"], m["nope"]);

// The other families' fixed keys keep answering.
const ab: any = new ArrayBuffer(8);
console.log(ab["byte" + "Length"], Reflect.get(ab, "byteLength"));
console.log([1, 2, 3]["len" + "gth"], "abc"["len" + "gth"]);
