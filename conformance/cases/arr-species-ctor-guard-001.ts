// RFC 20260713-array-proto-residual blade 3 — §9.4.2.3
// ArraySpeciesCreate constructor face: a present non-object
// non-undefined `constructor` expando throws TypeError on the
// species family (concat/filter/flat/map/slice/splice); absent /
// undefined / plain-object faces keep the default Array product;
// getPrototypeOf(arraylike-map product) is Array.prototype.
var a = [1];
(a as any).constructor = null;
try { a.concat([2]); console.log("concat no throw"); } catch (e) { console.log("concat threw:", e instanceof TypeError); }
var b = [1];
(b as any).constructor = 1;
try { b.slice(0); console.log("slice no throw"); } catch (e) { console.log("slice threw:", e instanceof TypeError); }
var c = [1];
(c as any).constructor = "string";
try { c.flat(); console.log("flat no throw"); } catch (e) { console.log("flat threw:", e instanceof TypeError); }
var d = [1];
(d as any).constructor = false;
try { d.splice(0, 1); console.log("splice no throw"); } catch (e) { console.log("splice threw:", e instanceof TypeError); }
var e2 = [1];
(e2 as any).constructor = null;
try { e2.map(function (x) { return x; }); console.log("map no throw"); } catch (er) { console.log("map threw:", er instanceof TypeError); }
var f2 = [1];
(f2 as any).constructor = null;
try { f2.filter(function (x) { return true; }); console.log("filter no throw"); } catch (er) { console.log("filter threw:", er instanceof TypeError); }
var g = [1, 2];
console.log("plain slice:", g.slice(1)[0]);
(g as any).constructor = {};
console.log("obj ctor slice:", g.slice(0)[0]);
(g as any).constructor = undefined;
console.log("undef ctor concat:", g.concat([3])[2]);
var obj: any = { length: 0 };
var result = Array.prototype.map.call(obj, function () {});
console.log("proto:", Object.getPrototypeOf(result) === Array.prototype);
console.log("arr proto:", Object.getPrototypeOf([1]) === Array.prototype);
