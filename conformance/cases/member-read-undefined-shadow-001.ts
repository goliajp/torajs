// Own-undefined shadow on the READ lanes — an own entry storing
// `undefined` shadows the inherited builtin surface (`get_tag`
// answers 5 for both stored-undefined and absent; the has probe
// disambiguates — the read-side leg of 777e756c's method-call fix).
// Pre-fix `arr.join = undefined; arr.join` read back the reified
// builtin cell instead of undefined.

function readKey(o: any, k: string): any {
  return o[k];
}

// dynobj: static + dynamic reads
const o: any = { toString: undefined, a: 1 };
console.log(o.toString); // undefined
console.log(readKey(o, "toString")); // undefined

// arr expando
const arr: any = [1, 2];
arr.join = undefined;
console.log(arr.join); // undefined
console.log(readKey(arr, "join")); // undefined

// optional call short-circuits on the shadow
console.log(arr.join?.()); // undefined

// closure expando
const f: any = (x: any) => x;
f.call = undefined;
console.log(f.call); // undefined
console.log(readKey(f, "call")); // undefined

// wrapper expando
const w: any = new Number(3);
w.toFixed = undefined;
console.log(w.toFixed); // undefined
console.log(readKey(w, "toFixed")); // undefined

// non-shadowed builtins still reify
const arr2: any = [3, 4];
console.log(arr2.join("-")); // 3-4
const w2: any = new Number(3.14159);
console.log(w2.toFixed(2)); // 3.14
console.log("done");
