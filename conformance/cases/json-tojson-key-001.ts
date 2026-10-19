// a toJSON hook the runtime walk calls receives the key its value sits
// under (§25.5.2.2 step 2): "" at the root, the property name in an
// object, the index as a string in an array
function hook(this: any, key: any) {
  return arguments.length + ":" + typeof key + ":" + key;
}

var top = { p: { toJSON: function (key: any) { return [arguments.length, key]; } } };
console.log(JSON.stringify(top));

const root: any = { toJSON: hook };
console.log(JSON.stringify(root));

const bag: any = {};
bag.first = { toJSON: hook };
bag["second key"] = { toJSON: hook };
console.log(JSON.stringify(bag));

const arr: any = [{ toJSON: hook }, 1, { toJSON: hook }];
console.log(JSON.stringify(arr));

const nested: any = { outer: [{ inner: { toJSON: hook } }] };
console.log(JSON.stringify(nested));

const listed: any = { a: { toJSON: hook }, b: 2 };
console.log(JSON.stringify(listed, ["a"]));
console.log(JSON.stringify(listed, null, 1));

const replaced: any = { r: { toJSON: hook } };
console.log(JSON.stringify(replaced, (k, v) => v));

// a deleted hook is gone, a symbol key named "toJSON" is no hook, and
// a hook added again after a delete is found
const gone: any = { toJSON: hook, x: 1 };
delete gone.toJSON;
console.log(JSON.stringify(gone));
const sym: any = { [Symbol("toJSON")]: hook, y: 2 };
console.log(JSON.stringify(sym));
const back: any = { toJSON: hook, z: 3 };
delete back.toJSON;
back.toJSON = hook;
const outer: any = {};
outer.back = back;
console.log(JSON.stringify(outer));
