// Two functions each declaring a class of the SAME name. The computed
// members were the one family the name-keyed side tables got wrong
// (RFC 20260921); these six surfaces were right all along and this
// pins them there, because every one of them also reads a table keyed
// by class name somewhere.
class Base1 { who() { return "B1"; } }
class Base2 { who() { return "B2"; } }
function priv1() { class K { #v = 1; get v() { return this.#v; } } return new K().v; }
function priv2() { class K { #v = 2; get v() { return this.#v; } } return new K().v; }
console.log("priv", priv1(), priv2());
function acc1() { class K { get g() { return "g1"; } set g(_: string) {} } return new K().g; }
function acc2() { class K { get g() { return "g2"; } } return new K().g; }
console.log("accessor", acc1(), acc2());
function ext1() { class K extends Base1 { m() { return super.who(); } } return new K().m(); }
function ext2() { class K extends Base2 { m() { return super.who(); } } return new K().m(); }
console.log("extends", ext1(), ext2());
function sm1() { class K { static s() { return "s1"; } } return K.s(); }
function sm2() { class K { static s() { return "s2"; } } return K.s(); }
console.log("static method", sm1(), sm2());
function sb1() { class K { static { (K as any).blk = "b1"; } } return (K as any).blk; }
function sb2() { class K { static { (K as any).blk = "b2"; } } return (K as any).blk; }
console.log("static block", sb1(), sb2());
function fd1() { class K { x = 1; } return new K().x; }
function fd2() { class K { x = 2; y = 3; } const k = new K(); return k.x + "," + k.y; }
console.log("fields", fd1(), fd2());
