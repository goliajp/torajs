// A literal stored under an object-type annotation whose own method
// writes its callback's type calls the callback as that type says, not
// as the field's type does.

type R = { run: (f: (x: number) => void, g: (y: number) => void) => void };

const own: R = {
  run(f: Function, g) {
    f("u");
    g(2);
  },
};
own.run(
  (x) => console.log("own", typeof x, x, x + 1),
  (y) => console.log("field", typeof y, y + 1),
);
