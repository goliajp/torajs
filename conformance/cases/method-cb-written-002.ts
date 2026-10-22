// An arrow handed to a method of an object literal takes the
// literal's parameter types only while nothing replaces the method:
// here the property is written.

const p = {
  go(f: (x: number) => void) {
    f(3);
  },
};
p.go = (f: (x: any) => void) => f(true);
p.go((x) => console.log("written", typeof x, x));
