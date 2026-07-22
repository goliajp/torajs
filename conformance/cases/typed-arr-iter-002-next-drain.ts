// via-var .next() on typed arrays (full drain) + object-array for-of isolated
const nums = [100, 200];
const it = nums.values();
const r1 = it.next(); const r2 = it.next(); const r3 = it.next();
console.log("drain:", r1.value, r2.value, r3.value, r3.done);
const strs: string[] = ["x", "y"];
const sit = strs.entries();
console.log("entries-next:", JSON.stringify(sit.next().value));
console.log("keys-count:", [...[1, 2, 3, 4].keys()].length);
