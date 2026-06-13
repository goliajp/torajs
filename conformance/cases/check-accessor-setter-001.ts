// RFC C3c — setter SET dispatch + getOwnPropertyDescriptor accessor
// reporting. Assigning to an accessor invokes the setter (never a data
// write); a getter-only accessor throws on assignment; the descriptor
// reports the accessor shape ({ get, set, enumerable, configurable }).

// setter invoked on assignment; getter reads it back through capture.
let stored = 0;
const o: any = {};
Object.defineProperty(o, "x", {
  get() {
    return stored;
  },
  set(v) {
    stored = v;
  },
});
o.x = 42;
console.log(o.x); // 42
console.log(stored); // 42

// getter-only accessor — assignment throws.
const o2: any = {};
Object.defineProperty(o2, "ro", {
  get() {
    return 1;
  },
});
let threw = false;
try {
  o2.ro = 5;
} catch (e) {
  threw = true;
}
console.log(threw); // true

// getOwnPropertyDescriptor reports the accessor shape.
const o3: any = {};
Object.defineProperty(o3, "a", {
  get() {
    return 7;
  },
  enumerable: true,
  configurable: true,
});
const d = Object.getOwnPropertyDescriptor(o3, "a");
console.log(typeof d.get); // function
console.log(d.set); // undefined
console.log(d.enumerable); // true
console.log(d.configurable); // true
