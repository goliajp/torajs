// A key the layout never mentions is an own property too. The
// surfaces that unfold a compile-time member list have to notice it,
// which they do through the same header test that catches a redefined
// declared member — a second bit, one masked compare.

function mk(): { a: number; b: number } {
  return { a: 1, b: 2 };
}

const z = mk();
Object.defineProperty(z as any, "extra", {
  value: 9,
  enumerable: true,
  configurable: true,
  writable: true,
});
console.log(Object.keys(z).join(","));
console.log(Object.entries(z).map((e) => e[0] + "=" + e[1]).join(" "));
console.log(JSON.stringify(z));
console.log(Object.getOwnPropertyNames(z).join(","), "extra" in z);

// A non-enumerable expando is own but not enumerated.
const y = mk();
Object.defineProperty(y as any, "hidden", { value: 4, enumerable: false });
console.log(Object.keys(y).join(","));
console.log(Object.entries(y).map((e) => e[0]).join(","));
console.log(JSON.stringify(y), (y as any).hidden);

// The declared members keep their own attributes independently.
const x = mk();
Object.defineProperty(x as any, "extra", { value: 3, enumerable: true });
Object.defineProperty(x as any, "a", { enumerable: false });
console.log(Object.entries(x).map((e) => e[0] + "=" + e[1]).join(" "));
console.log(JSON.stringify(x), x.a);

// An instance with neither still takes the plain unfold.
const q = mk();
console.log(Object.entries(q).map((e) => e[0]).join(","), JSON.stringify(q), Object.values(q).join(","));
