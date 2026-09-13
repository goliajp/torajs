// `yield* src` in an async generator runs the delegation protocol
// (§27.6.3.9): `@@asyncIterator` read once, `next` read once and called
// with one argument (undefined first, then the value the outer next(v)
// sent), the step awaited before `done` / `value` are read, a `throw(e)`
// resumption forwarded to the inner iterator's `throw`, a sync iterable
// taken through the Async-from-Sync posture. The log below is the
// object's own view of that, the shape every test262 `yield-star-*`
// ordering case reads.
function dump(tag: string, log: any[]) { console.log(tag, log.length, log.map((e: any) => e.name).join(" | ")); }

const log1: any[] = [];
const obj1: any = {
  get [Symbol.iterator]() {
    log1.push({ name: "get [Symbol.iterator]" });
  },
  get [Symbol.asyncIterator]() {
    log1.push({
      name: "get [Symbol.asyncIterator]",
      thisValue: this
    });
    return function() {
      log1.push({
        name: "call [Symbol.asyncIterator]",
        thisValue: this,
        args: [...arguments]
      });
      var nextCount = 0;
      return {
        name: "asyncIterator",
        get next() {
          log1.push({
            name: "get next",
            thisValue: this
          });
          return function() {
            log1.push({
              name: "call next",
              thisValue: this,
              args: [...arguments]
            });

            nextCount++;
            if (nextCount == 1) {
              return {
                name: "next-promise-1",
                get then() {
                  log1.push({
                    name: "get next then (1)",
                    thisValue: this
                  });
                  return function(resolve) {
                    log1.push({
                      name: "call next then (1)",
                      thisValue: this,
                      args: [...arguments]
                    });

                    resolve({
                      name: "next-result-1",
                      get value() {
                        log1.push({
                          name: "get next value (1)",
                          thisValue: this
                        });
                        return "next-value-1";
                      },
                      get done() {
                        log1.push({
                          name: "get next done (1)",
                          thisValue: this
                        });
                        return false;
                      }
                    });
                  };
                }
              };
            }

            return {
              name: "next-promise-2",
              get then() {
                log1.push({
                  name: "get next then (2)",
                  thisValue: this
                });
                return function(resolve) {
                  log1.push({
                    name: "call next then (2)",
                    thisValue: this,
                    args: [...arguments]
                  });

                  resolve({
                    name: "next-result-2",
                    get value() {
                      log1.push({
                        name: "get next value (2)",
                        thisValue: this
                      });
                      return "next-value-2";
                    },
                    get done() {
                      log1.push({
                        name: "get next done (2)",
                        thisValue: this
                      });
                      return true;
                    }
                  });
                };
              }
            };
          };
        }
      };
    };
  }
};



let callCount1 = 0;

async function* gen1() {
  callCount1 += 1;
  log1.push({ name: "before yield*" });
    var v = yield* obj1;
    log1.push({
      name: "after yield*",
      value: v
    });
    return "return-value";

}


const log2: any[] = [];
const obj2: any = {
  [Symbol.asyncIterator]() {
    var throwCount = 0;
    return {
      name: "asyncIterator",
      get next() {
        log2.push({ name: "get next" });
        return function() {
          return {
            value: "next-value-1",
            done: false
          };
        };
      },
      get throw() {
        log2.push({
          name: "get throw",
          thisValue: this
        });
        return function() {
          log2.push({
            name: "call throw",
            thisValue: this,
            args: [...arguments]
          });

          throwCount++;
          if (throwCount == 1) {
            return {
              name: "throw-promise-1",
              get then() {
                log2.push({
                  name: "get throw then (1)",
                  thisValue: this
                });
                return function(resolve) {
                  log2.push({
                    name: "call throw then (1)",
                    thisValue: this,
                    args: [...arguments]
                  });

                  resolve({
                    name: "throw-result-1",
                    get value() {
                      log2.push({
                        name: "get throw value (1)",
                        thisValue: this
                      });
                      return "throw-value-1";
                    },
                    get done() {
                      log2.push({
                        name: "get throw done (1)",
                        thisValue: this
                      });
                      return false;
                    }
                  });
                };
              }
            };
          }

          return {
            name: "throw-promise-2",
            get then() {
              log2.push({
                name: "get throw then (2)",
                thisValue: this
              });
              return function(resolve) {
                log2.push({
                  name: "call throw then (2)",
                  thisValue: this,
                  args: [...arguments]
                });

                resolve({
                  name: "throw-result-2",
                  get value() {
                    log2.push({
                      name: "get throw value (2)",
                      thisValue: this
                    });
                    return "throw-value-2";
                  },
                  get done() {
                    log2.push({
                      name: "get throw done (2)",
                      thisValue: this
                    });
                    return true;
                  }
                });
              };
            }
          };
        };
      }
    };
  }
};



let callCount2 = 0;

async function* gen2() {
  callCount2 += 1;
  log2.push({ name: "before yield*" });
    var v = yield* obj2;
    log2.push({
      name: "after yield*",
      value: v
    });
    return "return-value";

}


const log3: any[] = [];
const obj3: any = {
  get [Symbol.iterator]() {
    log3.push({
      name: "get [Symbol.iterator]",
      thisValue: this
    });
    return function() {
      log3.push({
        name: "call [Symbol.iterator]",
        thisValue: this,
        args: [...arguments]
      });
      var nextCount = 0;
      return {
        name: "syncIterator",
        get next() {
          log3.push({
            name: "get next",
            thisValue: this
          });
          return function() {
            log3.push({
              name: "call next",
              thisValue: this,
              args: [...arguments]
            });

            nextCount++;
            if (nextCount == 1) {
              return {
                name: "next-result-1",
                get value() {
                  log3.push({
                    name: "get next value (1)",
                    thisValue: this
                  });
                  return "next-value-1";
                },
                get done() {
                  log3.push({
                    name: "get next done (1)",
                    thisValue: this
                  });
                  return false;
                }
              };
            }

            return {
              name: "next-result-2",
              get value() {
                log3.push({
                  name: "get next value (2)",
                  thisValue: this
                });
                return "next-value-2";
              },
              get done() {
                log3.push({
                  name: "get next done (2)",
                  thisValue: this
                });
                return true;
              }
            };
          };
        }
      };
    };
  },
  get [Symbol.asyncIterator]() {
    log3.push({ name: "get [Symbol.asyncIterator]" });
    return null;
  }
};



let callCount3 = 0;

async function* gen3() {
  callCount3 += 1;
  log3.push({ name: "before yield*" });
    var v = yield* obj3;
    log3.push({
      name: "after yield*",
      value: v
    });
    return "return-value";

}

async function main() {
  let it: any = gen1();
  let v: any = await it.next("next-arg-1"); dump("async-next s1", log1); console.log("v1", v.value, v.done);
  v = await it.next("next-arg-2"); dump("async-next s2", log1); console.log("v2", v.value, v.done);
  it = gen2();
  v = await it.next("next-arg-1"); dump("async-throw s1", log2);
  v = await it.throw("throw-arg-1"); dump("async-throw s2", log2); console.log("v2", v.value, v.done);
  it = gen3();
  v = await it.next("next-arg-1"); dump("sync-next s1", log3); console.log("v1", v.value, v.done);
  v = await it.next("next-arg-2"); dump("sync-next s2", log3); console.log("v2", v.value, v.done);
}
main();
