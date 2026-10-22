// A literal stored under an object-type annotation whose own method
// takes its callback as `any` may call it with anything.

type R = { run: (f: (x: number) => void) => void };

const anyParam: R = {
  run(f: any) {
    f("u");
  },
};
anyParam.run((x) => console.log("any-param", typeof x, x));
