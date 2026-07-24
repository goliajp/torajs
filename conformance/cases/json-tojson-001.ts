// Rotation 205 — sec 25.5.2.3 SerializeJSONProperty step 2: the user
// toJSON hook. A DynObj (or an Array's expando props) carrying a
// callable toJSON serializes the hook's RESULT; a hook answering a
// cyclic structure lands on the recursion cap's TypeError.

// face 1 — hook consulted at top level, in object fields, and in
// array elements.
var d = {};
d.v = 42;
d.toJSON = function () {
  return { w: 43 };
};
console.log(JSON.stringify(d));
console.log(JSON.stringify({ d: d }));
console.log(JSON.stringify([d]));

// face 2 — toJSON returning a circular object throws TypeError
// (test262 value-tojson-object-circular shape; the holder is a
// struct-lane literal, so the struct field walk consults the hook).
var obj = {};
var circular = { prop: obj };
obj.toJSON = function () {
  return circular;
};
try {
  JSON.stringify(circular);
  console.log("no-throw");
} catch (e) {
  console.log("caught:", e.constructor.name);
}

// face 3 — array expando toJSON + circular array (value-tojson-array-
// circular shape).
var arr = [];
var c2 = [arr];
arr.toJSON = function () {
  return c2;
};
try {
  JSON.stringify(c2);
  console.log("no-throw2");
} catch (e) {
  console.log("caught2:", e.constructor.name);
}

// face 4 — non-object hook results serialize as their type.
var n = {};
n.toJSON = function () {
  return 7;
};
console.log(JSON.stringify({ n: n }));
