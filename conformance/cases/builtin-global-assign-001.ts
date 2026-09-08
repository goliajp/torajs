// §10.2.4 puts the built-in constructors and namespace objects on the
// global object as ordinary WRITABLE properties, so a program may
// replace one — test262 does it constantly, installing a throwing
// stand-in to prove an internal read does not go through the public
// name. tr resolves those names at compile time, so the write had
// nowhere to land: `assignment to undeclared \`Array\``. Reading one
// as a value already worked, which is the whole shape needed.
const oldArray = Array;
const oldObject = Object;

Array = function () {
  throw new Error("invoke customer defined Array!");
} as any;

// the engine's own internals do not go through the public name
const names = oldObject.getOwnPropertyNames({ a: 1, b: 2 });
console.log(names.length, names[0], names[1]);
console.log(oldObject.prototype.toString.call(names));

console.log(typeof Array);
console.log(Array === oldArray);
let threw = "";
try {
  (Array as any)();
} catch (e: any) {
  threw = String(e.message);
}
console.log(threw);

// restoring the original puts every use back
Array = oldArray as any;
console.log(Array === oldArray);
const back = new Array(3);
console.log(back.length);
console.log(Array.isArray(back));
console.log([1, 2] instanceof Array);

// a second built-in demotes independently
Object = function () {
  return "shadowed";
} as any;
console.log((Object as any)());
console.log(typeof oldObject.keys({ x: 1 }));
