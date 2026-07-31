// §28.1.2 Reflect.defineProperty — accessor-descriptor half (R5b):
// a §10.1.6.3 refusal on an accessor define answers false with no
// throw; ToPropertyDescriptor rejections (face mix, non-callable
// face) still throw for either flavor.
const o: any = {};
console.log(
  Reflect.defineProperty(o, "a", {
    get() {
      return 1;
    },
    configurable: false,
  }),
);
console.log(o.a);

// face change on a non-configurable accessor refuses with false
console.log(
  Reflect.defineProperty(o, "a", {
    get() {
      return 2;
    },
  }),
);
console.log(o.a);

// face mix still throws (ToPropertyDescriptor step 9/10)
try {
  Reflect.defineProperty(o, "b", {
    get() {
      return 1;
    },
    value: 2,
  });
} catch (e: any) {
  console.log("mix", e instanceof TypeError);
}

// non-callable getter still throws
try {
  Reflect.defineProperty(o, "c", { get: 5 as any });
} catch (e: any) {
  console.log("gcall", e instanceof TypeError);
}

// accessor fresh key on a frozen target refuses with false
const f: any = {};
Object.freeze(f);
console.log(
  Reflect.defineProperty(f, "y", {
    get() {
      return 9;
    },
  }),
);

// array accessor index: non-configurable face change refuses
const arr: any = [1];
Object.defineProperty(arr, "0", {
  get() {
    return 5;
  },
  configurable: false,
});
console.log(
  Reflect.defineProperty(arr, "0", {
    get() {
      return 6;
    },
  }),
);
console.log(arr[0]);

// data → accessor transition on a non-configurable data index refuses
const arr2: any = [1];
Object.defineProperty(arr2, "0", { writable: false, configurable: false });
console.log(
  Reflect.defineProperty(arr2, "0", {
    get() {
      return 7;
    },
  }),
);
console.log(arr2[0]);
