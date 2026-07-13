// RFC 20260714-t262-top-clusters 刀 3 — §8.10.5 ToPropertyDescriptor
// accepts ANY object: a function-with-expando descriptor (test262's
// `descObj = function(){}; descObj.configurable = true` idiom, 8.10.5
// step 4.a own [[Get]] reads) used to be rejected by the
// defineProperties/create per-prop gate ("Property description must
// be an object.") before it could reach define_from_desc's
// shape-dispatching reader.

var descObj = function () {};
descObj.configurable = true;

// Object.create with a fn descriptor — configurable so delete works
var newObj = Object.create({}, { prop: descObj });
var r1 = newObj.hasOwnProperty("prop");
delete newObj.prop;
console.log(r1, newObj.hasOwnProperty("prop"));

// Object.defineProperty single form
var o2: any = {};
Object.defineProperty(o2, "q", descObj);
console.log(o2.hasOwnProperty("q"));

// Object.defineProperties bag form
var o3: any = {};
Object.defineProperties(o3, { r: descObj });
console.log(o3.hasOwnProperty("r"));

// value-bearing fn descriptor
var vd = function () {};
vd.value = 12;
vd.enumerable = true;
var o4: any = {};
Object.defineProperty(o4, "v", vd);
console.log(o4.v);

// a primitive descriptor still throws
try {
  Object.defineProperties({}, { p: 42 });
} catch (e) {
  console.log("primitive: caught");
}

console.log("done");
