// An arrow handed to a method of an object literal takes the
// literal's parameter types only while nothing replaces the method:
// here the binding is reassigned (through `any`, so the new method's
// own parameter types are not checked against the first literal's).

let o = {
  arm(f: (x: number) => void) {
    f(2);
  },
};
o = {
  arm(f: (x: string) => void) {
    f("str");
  },
} as any;
o.arm((x) => console.log("rebound", typeof x, x));
