// §23.1.3.18 join runs ToString on each ELEMENT, and for an object
// element that means the spec's OrdinaryToPrimitive walk — a real
// method call. The stub judgment decides which dispatch families a
// program can reach by scanning what the module calls, and the join
// kernel does its coercion per element: nothing in the user code
// mentions the coercion surface, so the object world was judged
// unreachable and its arms were replaced with reject stubs. Every
// element that needed one answered `undefined`.
const objs: any[] = [{ x: 1 }, { x: 2 }];
console.log(objs.join("|"));
console.log(objs.toString());
console.log(String(objs));

// the one that names it: a user toString the spec requires to run
const custom: any[] = [{ toString() { return "T"; } }];
console.log(custom.join(","));

// hint string asks toString first, so valueOf is not the answer here
const vo: any[] = [{ valueOf() { return 7; } }];
console.log(vo.join(","));

const nested: any[] = [[1, 2], [3]];
console.log(nested.join("-"));

const mixed: any[] = [1, "a", null, undefined, [2], { x: 1 }];
console.log(mixed.join("-"));

// lanes that never left: primitives, and the typed kernels
const nums: any[] = [1, 2];
console.log(nums.join("-"), [1, 2, 3].join("-"), String([1, 2, 3]));
console.log(["a", "b"].join("+"), [true, false].toString());
