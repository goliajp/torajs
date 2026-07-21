// §10.1.8.1 OrdinaryGet step 3 on digit keys (RFC
// 20260721-array-proto-cluster 刀 4a / G2d) — an own miss on a
// dynobj receiver continues to the user [[Prototype]] chain and the
// %Object.prototype% face; the array-like generics read inherited
// index props through it.
const anyIndexProto: any = { 0: "inherited-zero" };
const child: any = Object.create(anyIndexProto);
console.log(child[0]); // inherited-zero
console.log(child[1]); // undefined

// Grandparent chain.
const grand: any = Object.create(child);
console.log(grand[0]); // inherited-zero

// Own entry shadows the chain.
child[0] = "own-zero";
console.log(child[0]); // own-zero
console.log(grand[0]); // own-zero

// %Object.prototype% singleton digit keys are visible to plain
// objects and to the array-like generic reads.
(Object.prototype as any)[0] = true;
(Object.prototype as any)[1] = 41;
const target: any = { length: 2 };
console.log(target[0]); // true
console.log(target[1]); // 41
console.log(1 in target); // true — §7.3.11 rides the same chain
console.log(5 in target); // false
console.log(Array.prototype.indexOf.call(target, 41)); // 1
console.log(Array.prototype.lastIndexOf.call(target, true)); // 0
console.log(Array.prototype.indexOf.call(target, "missing")); // -1

// A primitive receiver reads its wrapper prototype's expando
// length + digit keys (Boolean.prototype face, 刀 4b).
const bproto: any = Boolean.prototype;
bproto[1] = "bool-one";
bproto.length = 2;
console.log(Array.prototype.indexOf.call(true, "bool-one")); // 1
console.log(Array.prototype.lastIndexOf.call(false, "bool-one")); // 1
console.log(Array.prototype.indexOf.call(true, "absent")); // -1

// Accessor on the singleton runs with the reading receiver.
Object.defineProperty(Object.prototype, "2", {
  get() {
    return "acc2";
  },
  configurable: true,
});
const three: any = { length: 3 };
console.log(three[2]); // acc2
console.log(Array.prototype.indexOf.call(three, "acc2")); // 2
