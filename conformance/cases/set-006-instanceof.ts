// instanceof static-fold coverage for Map/Set/WeakMap/WeakSet — both
// the direct constructor check and the instanceof Object umbrella.

const m = new Map();
const s = new Set();
console.log(m instanceof Map, s instanceof Set, m instanceof Object, s instanceof Object);

const wm = new WeakMap();
const ws = new WeakSet();
console.log(wm instanceof WeakMap, ws instanceof WeakSet, wm instanceof Object, ws instanceof Object);

// cross-constructor stays false
console.log(m instanceof Set, s instanceof Map, m instanceof Array, s instanceof Date);

// ES2025 set-method return values are real Sets, and fresh objects
const a = new Set([1, 2]);
const b = new Set([2, 3]);
console.log(a.union(b) instanceof Set, a.difference(a) instanceof Set, a.difference(a) === a);
console.log(a.intersection(b) instanceof Set, a.symmetricDifference(b) instanceof Set);

// Any-boxed runtime dispatch agrees with the static fold
const am: any = m;
const asx: any = s;
console.log(am instanceof Map, asx instanceof Set, am instanceof Object, asx instanceof Object);
