// The data-global gate enumerated a subset of what the drop machinery
// already dispatches on, so every other refcounted slot type the
// CHECKER registers was registered on one side and refused on the
// other: these all typechecked and then died in lowering with
// "unknown ident". Both sides now ask the same question.
let m: Map<string, number> = new Map();
function mapSize() { return m.size }
console.log("map", mapSize());

let s: Set<number> = new Set();
function setSize() { return s.size }
console.log("set", setSize());

let d: Date = new Date(0);
function time() { return d.getTime() }
console.log("date", time());

let re: RegExp = /ab/;
function source() { return re.source }
console.log("regexp", source());

let p: Promise<number> = Promise.resolve(1);
function take() { return p }
take().then(v => console.log("promise", v));

let big: bigint = 1n;
function inc() { return big + 1n }
console.log("bigint", inc().toString());

let wm: WeakMap<object, number> = new WeakMap();
function kind() { return typeof wm }
console.log("weakmap", kind());

// A binding with NO named-fn reader stays main-local: it already
// works there, method calls included, and the global path's
// member-call lane does not carry these types.
const local: Map<string, number> = new Map();
local.set("a", 1);
console.log("stays-local", local.size, local.get("a"));
