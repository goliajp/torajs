import { viaComputed, viaGen, viaSuper, viaAsync, viaStatic, viaAnn } from "./lib_shapes";
class C2 { }
class G0 { }
class P0 { m() { return 40; } }
class S0 extends P0 { }
class A0 { }
class S1 { static sv = 1; }
class T1 { n = 99; }
console.log(viaComputed(), viaGen(), viaSuper(), viaStatic(), viaAnn());
console.log(new S0().m(), S1.sv, new T1().n);
viaAsync().then((v) => console.log(v));
