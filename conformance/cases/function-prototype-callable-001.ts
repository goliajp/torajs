// §20.2.3 — %Function.prototype% is itself a function object: called
// with any arguments it returns undefined, through every spelling.
console.log(Function.prototype(), Function.prototype(null, void 0), Function.prototype(1, "a", {}));
var fp: any = Function.prototype;
console.log(fp(), fp.call(null, 3), fp.apply(undefined, [1, 2]), typeof fp, fp.length, JSON.stringify(fp.name));
try { (Array as any).prototype(); console.log("no throw"); } catch (e) { console.log("Array.prototype():", e instanceof TypeError); }
try { (Object as any).prototype(); console.log("no throw"); } catch (e) { console.log("Object.prototype():", e instanceof TypeError); }
