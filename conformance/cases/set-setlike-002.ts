// ES2025 §24.2.1.2 GetSetRecord on the TYPED tier — the checker admits
// any argument to the seven Set methods; a statically-Set argument keeps
// the Set×Set fast kernels, everything else walks the runtime protocol.
const s = new Set([1, 2, 3]);

// Set×Set fast path unchanged
const t = new Set([2, 3, 4]);
console.log([...s.union(t)].join(","));
console.log([...s.intersection(t)].join(","));
console.log(s.isSubsetOf(t), s.isSupersetOf(t), s.isDisjointFrom(t));

// object-literal set-like (typed struct argument)
const like = {
  size: 2,
  has: (v: any) => v === 1 || v === 2,
  keys: () => [1, 2][Symbol.iterator](),
};
console.log([...s.union(like)].join(","));
console.log([...s.intersection(like)].join(","));
console.log([...s.difference(like)].join(","));
console.log([...s.symmetricDifference(like)].join(","));
console.log(s.isSubsetOf(like), s.isSupersetOf(like), s.isDisjointFrom(like));

// Map argument (set-like over its keys)
const m = new Map([[1, "a"], [9, "b"]]);
console.log([...s.intersection(m)].join(","));
console.log([...s.union(m)].join(","));

// class instance with an accessor size
class SL {
  get size() { return 1; }
  has(v: any) { return v === 3; }
  keys() { return [3][Symbol.iterator](); }
}
console.log([...s.intersection(new SL())].join(","));

// inline literal argument (owned temp settles cleanly)
console.log(s.isSubsetOf({ size: 9, has: () => true, keys: () => [][Symbol.iterator]() }));

// refusals reach the runtime as catchable spec errors
try { s.union([1, 2]); } catch (e) { console.log("arr:", (e as Error).name); }
try { s.union(5 as any); } catch (e) { console.log("num:", (e as Error).name); }
try { s.difference({ size: NaN, has: () => true, keys: () => [][Symbol.iterator]() }); } catch (e) { console.log("nan:", (e as Error).name); }
try { s.intersection({ size: -1, has: () => true, keys: () => [][Symbol.iterator]() }); } catch (e) { console.log("neg:", (e as Error).name); }
try { s.isDisjointFrom({ size: 1, has: 5 as any, keys: () => [][Symbol.iterator]() }); } catch (e) { console.log("has:", (e as Error).name); }

// a throwing has propagates through the typed route
try {
  s.isSubsetOf({ size: 9, has: () => { throw new RangeError("boom"); }, keys: () => [][Symbol.iterator]() });
} catch (e) { console.log("throw-has:", (e as Error).name, (e as Error).message); }

// receiver unchanged
console.log([...s].join(","));
console.log("done");
