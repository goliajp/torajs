// ES2025 §24.2.1.2 GetSetRecord — Set methods accept set-like arguments
// on the any tier: object literals, classes with accessor size, Maps;
// primitives and size-less objects refuse per spec.
const s = new Set([1, 2, 3]);
const sa: any = s;

const like: any = {
  size: 2,
  has: (v: any) => v === 1 || v === 2,
  keys: () => [1, 2][Symbol.iterator](),
};

console.log([...sa.union(like)].join(","));
console.log([...sa.intersection(like)].join(","));
console.log([...sa.difference(like)].join(","));
console.log([...sa.symmetricDifference(like)].join(","));
console.log(sa.isSubsetOf(like));
console.log(sa.isSupersetOf(like));
console.log(sa.isDisjointFrom(like));

// subset / superset / disjoint verdicts across size splits
console.log(sa.isSubsetOf({ size: 9, has: () => true, keys: () => [][Symbol.iterator]() }));
console.log(sa.isSupersetOf({ size: 0, has: () => false, keys: () => [][Symbol.iterator]() }));
console.log(sa.isDisjointFrom({ size: 9, has: (v: any) => v === 99, keys: () => [][Symbol.iterator]() }));

// a Map is set-like over its keys
const m = new Map([[1, "a"], [9, "b"]]);
console.log([...sa.intersection(m)].join(","));
console.log([...sa.union(m)].join(","));

// a class with a getter size and prototype methods
class SL {
  get size() { return 1; }
  has(v: any) { return v === 3; }
  keys() { return [3][Symbol.iterator](); }
}
console.log([...sa.intersection(new SL())].join(","));

// -0 from the keys iterator canonicalizes to +0
const negz: any = { size: 1, has: (v: any) => v === 0, keys: () => [-0][Symbol.iterator]() };
console.log([...sa.union(negz)].join(","));

// refusals — array (absent size → NaN), primitive, NaN size,
// negative size, non-callable has
try { sa.union([1, 2]); } catch (e) { console.log("arr:", (e as Error).name); }
try { sa.union(5); } catch (e) { console.log("num:", (e as Error).name); }
try { sa.union("ab"); } catch (e) { console.log("str:", (e as Error).name); }
try { sa.union({ size: NaN, has: () => true, keys: () => [][Symbol.iterator]() }); } catch (e) { console.log("nan:", (e as Error).name); }
try { sa.union({ size: -1, has: () => true, keys: () => [][Symbol.iterator]() }); } catch (e) { console.log("neg:", (e as Error).name); }
try { sa.union({ size: 1, has: 5, keys: () => [][Symbol.iterator]() }); } catch (e) { console.log("has:", (e as Error).name); }
try { sa.union({ size: 1, has: () => true, keys: undefined }); } catch (e) { console.log("keys:", (e as Error).name); }

// a throwing has propagates and the walk stops
try {
  sa.isSubsetOf({ size: 9, has: () => { throw new RangeError("boom"); }, keys: () => [][Symbol.iterator]() });
} catch (e) { console.log("throw-has:", (e as Error).name, (e as Error).message); }

// receiver unchanged by every walk above
console.log([...sa].join(","));
console.log("done");
