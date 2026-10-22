// An arrow handed to a method of a receiver annotated with an object
// type takes the field's parameter types only from the literal the
// receiver is initialized with; another value stored under the
// annotation runs its own method, which may call the arrow with
// anything.

type R = { run: (f: (x: number) => void) => void };

const impl = {
  run(f: (x: any) => void) {
    f("s");
  },
};

const o: R = impl as any;
o.run((x) => console.log(typeof x, x));

function viaParam(p: R) {
  p.run((x) => console.log("param", typeof x, x));
}
viaParam(impl as any);

const lit: R = {
  run(f) {
    f(41);
  },
};
lit.run((x) => console.log("literal", typeof x, x + 1));

const lit2: R = {
  run: (f) => f(7),
};
lit2.run((x) => console.log("arrow", typeof x, x * 2));

const anyParam: R = {
  run(f: any) {
    f("u");
  },
};
anyParam.run((x) => console.log("any-param", typeof x, x));
