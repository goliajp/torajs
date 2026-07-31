// §28.1.2 Reflect.defineProperty — boolean-answer define: success =
// true, §10.1.6.3 refusal = false (no throw); target must be an
// object (strict gate), desc must be an object, ToPropertyDescriptor
// throws propagate.
const o: any = {};
console.log(Reflect.defineProperty(o, "a", { value: 1, writable: true, enumerable: true, configurable: true }));
console.log(o.a);

// non-configurable redefine refuses with false, no throw
Reflect.defineProperty(o, "b", { value: 2 });
console.log(Reflect.defineProperty(o, "b", { value: 3 }));
console.log(o.b);

// same-value redefine of a readonly slot is compatible → true
console.log(Reflect.defineProperty(o, "b", { value: 2 }));

// frozen target refuses fresh keys
const f: any = Object.freeze({ x: 1 });
console.log(Reflect.defineProperty(f, "y", { value: 9 }));
console.log(f.y);

// primitive target throws TypeError (strict IsObject gate)
try {
  Reflect.defineProperty(1 as any, "k", { value: 1 });
} catch (e: any) {
  console.log("caught-target", e instanceof TypeError);
}

// non-object descriptor throws TypeError (§6.2.6.5 step 1)
try {
  Reflect.defineProperty(o, "k", 5 as any);
} catch (e: any) {
  console.log("caught-desc", e instanceof TypeError);
}

// array index define answers true and lands in element storage
const arr: any = [1, 2, 3];
console.log(Reflect.defineProperty(arr, "0", { value: 42 }));
console.log(arr[0]);

// locked length refuses a changed value with false, no throw
Object.defineProperty(arr, "length", { writable: false });
console.log(Reflect.defineProperty(arr, "length", { value: 10 }));
console.log(arr.length);

// a throwing getter-backed desc field propagates (ToPropertyDescriptor)
const badDesc: any = {};
Object.defineProperty(badDesc, "value", {
  get() {
    throw new TypeError("boom");
  },
});
try {
  Reflect.defineProperty(o, "z", badDesc);
} catch (e: any) {
  console.log("caught-getter", (e as Error).message);
}

// reflection face + detached call
const rdp: any = Reflect.defineProperty;
console.log(rdp.length, rdp.name);
const o2: any = {};
console.log(rdp(o2, "m", { value: 7, configurable: true }));
console.log(o2.m);

// .call form
console.log((Reflect.defineProperty as any).call(null, o2, "n", { value: 8 }));
console.log(o2.n);
