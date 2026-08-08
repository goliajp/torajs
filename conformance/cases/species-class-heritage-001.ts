// RFC 20260808-construct-channel 刀 4 + B5 — an `extends Array`
// class inherits `Array[@@species]`'s default getter (§23.1.2.5):
// `a.constructor = MyArr` alone routes ArraySpeciesCreate through
// Construct(MyArr, « len ») and the derive lands in a MyArr
// instance. The adapter-synthesis predicate arms on the
// `constructor` property write (no NewDynamic / Reflect.construct
// in sight).
class MyArr extends Array {}
var a = [1, 2, 3];
(a as any).constructor = MyArr;
var s = a.slice(1);
console.log(s instanceof MyArr);
console.log(s.length, s[0], s[1]);
var c = a.concat([4]);
console.log(c instanceof MyArr);
var aa: any = a;
var s2 = aa.slice(1);
console.log(s2 instanceof MyArr);
