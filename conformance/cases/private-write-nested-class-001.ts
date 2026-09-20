// §15.7 puts every PrivateName a class body declares into the scope of
// that entire body, nested classes included. A reference that resolves
// is therefore legal code; whether the receiver carries the brand is
// §7.3.32's question, answered at run time — PrivateFieldSet step 4
// throws a catchable TypeError on a receiver that does not.
//
// The read path has consulted the declared-name scope for a while. The
// write path had not, so the write below was refused outright: the
// same rule, implemented once.

class Outer {
    #x: number = 1;
    static make(): any {
        class Inner {
            bump(o: any): void {
                o.#x = 41;
            }
        }
        return new Inner();
    }
    read(): number {
        return this.#x;
    }
}

const o = new Outer();
Outer.make().bump(o);
console.log(o.read());

// A receiver without the brand reaches the runtime check, not a
// compile-time refusal.
try {
    Outer.make().bump({});
    console.log("no throw");
} catch (e: any) {
    console.log("brand:", e.constructor.name);
}
