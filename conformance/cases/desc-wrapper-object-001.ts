// §6.2.6.5 ToPropertyDescriptor accepts ANY object — a primitive
// WRAPPER cell (`new String()` carrying expando descriptor fields,
// the Object/create 15.2.3.5-4-11x family) is a true object; its
// fields read off the +16 lazy expando. Pre-fix the accept gates
// bundled wrappers with primitive Str and threw "Property
// description must be an object."

const descObj: any = new String();
descObj.configurable = true;
const newObj = Object.create({}, { prop: descObj });
console.log(newObj.hasOwnProperty("prop")); // true
delete newObj.prop;
console.log(newObj.hasOwnProperty("prop")); // false

// Boolean wrapper descriptor through defineProperty
const o: any = {};
const d2: any = new Boolean(false);
d2.value = 7;
Object.defineProperty(o, "k", d2);
console.log(o.k); // 7

// Number wrapper descriptor through defineProperties
const o2: any = {};
const d3: any = new Number(1);
d3.value = "v";
d3.enumerable = true;
Object.defineProperties(o2, { p: d3 });
console.log(o2.p); // v

// expando-free wrapper = all-absent empty descriptor
const o3: any = {};
Object.defineProperty(o3, "e", new String("x") as any);
console.log(o3.hasOwnProperty("e"), o3.e); // true undefined

// primitive string descriptor still rejects
try {
  Object.defineProperty({}, "bad", "notAnObject" as any);
  console.log("no throw");
} catch (e) {
  console.log("caught:", e instanceof TypeError);
}
console.log("done");
