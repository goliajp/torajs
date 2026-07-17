// The literal-descriptor pack collapsed `value: undefined` to the
// NULL tag (both are ConstPtrNull at the value layer), so
// `defineProperty(arr, "length", {value: undefined})` converted
// ToUint32(null)=0 == ToNumber(null)=0 and silently passed where
// §10.4.2.4 requires the RangeError (ToUint32(undefined)=0 !=
// ToNumber(undefined)=NaN). S127-1 twin: the checker's static type
// picks the tag.

const arrObj: any = [];
try {
  Object.defineProperty(arrObj, "length", { value: undefined });
  console.log("no throw");
} catch (e) {
  console.log("caught:", (e as any) instanceof RangeError);
}

// undefined data values stay undefined (not null) through define
const o: any = {};
Object.defineProperty(o, "u", { value: undefined, enumerable: true });
console.log(o.u === undefined, o.hasOwnProperty("u")); // true true

// null stays null
Object.defineProperty(o, "n", { value: null, enumerable: true });
console.log(o.n === null); // true

// length with a real number still works
Object.defineProperty(arrObj, "length", { value: 2 });
console.log(arrObj.length); // 2
console.log("done");
