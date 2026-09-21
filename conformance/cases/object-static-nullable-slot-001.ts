// §7.1.18 ToObject throws a TypeError for undefined, and a
// pointer-shaped slot is holding undefined whenever it holds the
// generic immortal cell. The typed arms of the three ToObject
// surfaces read it as a live value of their own type instead: the
// struct arm walks the compile-time layout, which is how
// `Object.values(o)` came back with whatever the field offset
// landed on.

const os: { a: number }[] = [];
const miss = os.find((x) => true);

try {
  Object.keys(miss);
  console.log("keys: no throw");
} catch (e) {
  console.log("keys:", (e as Error).constructor.name);
}
try {
  Object.values(miss);
  console.log("values: no throw");
} catch (e) {
  console.log("values:", (e as Error).constructor.name);
}
try {
  Object.entries(miss);
  console.log("entries: no throw");
} catch (e) {
  console.log("entries:", (e as Error).constructor.name);
}

// the annotated spelling of the same slot
const q: { a: number } | undefined = undefined;
try {
  Object.keys(q);
  console.log("q keys: no throw");
} catch (e) {
  console.log("q keys:", (e as Error).constructor.name);
}

// --- a live value still takes the compile-time layout walk
const hit = [{ a: 1, b: 2 }].find((x) => true);
console.log(Object.keys(hit).join(","));
console.log(Object.values(hit).join(","));
console.log(JSON.stringify(Object.entries(hit)));

const q2: { a: number } | undefined = { a: 7 };
console.log(Object.keys(q2).join(","));
console.log(Object.values(q2).join(","));

// --- the arms that were already right keep their shapes
console.log(Object.keys({ a: 1, b: 2 }).join(","));
console.log(Object.values({ a: 1, b: 2 }).join(","));
console.log(JSON.stringify(Object.entries({ a: 1 })));
console.log(Object.keys([9, 8]).join(","));
console.log(Object.values([9, 8]).join(","));
console.log(Object.keys("ab").join(","));
