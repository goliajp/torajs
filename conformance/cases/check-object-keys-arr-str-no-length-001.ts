// `Object.keys(arr)` / `Object.keys(str)` filter to enumerable-own per
// spec §22.1.3.16 + §10.4.2.4 step 4 (Array `length` non-enumerable)
// + §22.1.5.1 (String `length` non-enumerable), so the returned key
// list omits the trailing "length" that `getOwnPropertyNames` /
// `Reflect.ownKeys` include. Before this fix tr emitted
// `["0", ..., "<len-1>", "length"]` uniformly for all three surfaces,
// silently surfacing a non-enumerable key into the enumerable-only
// view.

const a: number[] = [10, 20, 30];
console.log("Object.keys(arr):", JSON.stringify(Object.keys(a)));
console.log("Object.getOwnPropertyNames(arr):", JSON.stringify(Object.getOwnPropertyNames(a)));
console.log("Reflect.ownKeys(arr):", JSON.stringify(Reflect.ownKeys(a)));

const s = "abc";
console.log("Object.keys(str):", JSON.stringify(Object.keys(s)));
console.log("Object.getOwnPropertyNames(str):", JSON.stringify(Object.getOwnPropertyNames(s)));

const empty: number[] = [];
console.log("Object.keys([]):", JSON.stringify(Object.keys(empty)));
console.log("Object.getOwnPropertyNames([]):", JSON.stringify(Object.getOwnPropertyNames(empty)));

const emptyStr = "";
console.log("Object.keys(''):", JSON.stringify(Object.keys(emptyStr)));
console.log("Object.getOwnPropertyNames(''):", JSON.stringify(Object.getOwnPropertyNames(emptyStr)));

const strArr: string[] = ["x", "y"];
console.log("Object.keys(strArr):", JSON.stringify(Object.keys(strArr)));
console.log("Object.getOwnPropertyNames(strArr):", JSON.stringify(Object.getOwnPropertyNames(strArr)));
