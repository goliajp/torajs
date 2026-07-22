// inferred + annotated typed arrays, full for-of consumption (safe)
const inf = [10, 20, 30];
let a = ""; for (const v of inf.values()) a += v + ","; console.log("inf-values:", a);
let e = ""; for (const [i, v] of inf.entries()) e += `${i}:${v};`; console.log("inf-entries:", e);
let k = ""; for (const idx of inf.keys()) k += idx + ","; console.log("inf-keys:", k);
const ann: string[] = ["p", "q", "r"];
let s = ""; for (const v of ann.values()) s += v; console.log("ann-str-values:", s);
console.log("inf-spread:", [...inf.values()].join("-"));
const bools = [true, false, true];
console.log("bool-values:", [...bools.values()].join(","));
