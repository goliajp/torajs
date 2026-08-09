// rotation 346 — seventh receiver-safe face position: a marked
// fn-expr RETURNED from an objlit accessor body. The returned
// function's `this` binds at ITS call site (§10.2.1.2), not to the
// accessor receiver — the promote gives it the receiver param, so a
// `.call` with a different receiver observes the right object.
const resource: any = {
  disposed: false,
  get f() {
    return function () {
      this.disposed = true;
    };
  },
};
const g: any = resource.f;
const other: any = { disposed: false };
g.call(other);
console.log(resource.disposed);
console.log(other.disposed);
g.call(resource);
console.log(resource.disposed);
