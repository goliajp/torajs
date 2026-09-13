// `yield* src` in an async generator runs the delegation protocol
// (§27.6.3.9): `@@asyncIterator` read once, `next` read once and called
// with one argument (undefined first, then the value the outer next(v)
// sent), the step awaited before `done` / `value` are read, a `throw(e)`
// or `return(v)` resumption forwarded to the inner iterator's `throw` /
// `return` (a return request resumes the delegating yield as a return
// completion — `this.__ysact` / `this.__ysret`), a sync iterable taken
// through the Async-from-Sync posture. The log below is the
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


const log4: any[] = [];
const obj4: any = {
  [Symbol.asyncIterator]() {
    var returnCount = 0;
    return {
      name: 'asyncIterator',
      get next() {
        log4.push({ name: "get next" });
        return function() {
          return {
            value: "next-value-1",
            done: false
          };
        };
      },
      get return() {
        log4.push({
          name: "get return",
          thisValue: this
        });
        return function() {
          log4.push({
            name: "call return",
            thisValue: this,
            args: [...arguments]
          });

          returnCount++;
          if (returnCount == 1) {
            return {
              name: "return-promise-1",
              get then() {
                log4.push({
                  name: "get return then (1)",
                  thisValue: this
                });
                return function(resolve) {
                  log4.push({
                    name: "call return then (1)",
                    thisValue: this,
                    args: [...arguments]
                  });

                  resolve({
                    name: "return-result-1",
                    get value() {
                      log4.push({
                        name: "get return value (1)",
                        thisValue: this
                      });
                      return "return-value-1";
                    },
                    get done() {
                      log4.push({
                        name: "get return done (1)",
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
            name: "return-promise-2",
            get then() {
              log4.push({
                name: "get return then (2)",
                thisValue: this
              });
              return function(resolve) {
                log4.push({
                  name: "call return then (2)",
                  thisValue: this,
                  args: [...arguments]
                });

                resolve({
                  name: "return-result-2",
                  get value() {
                    log4.push({
                      name: "get return value (2)",
                      thisValue: this
                    });
                    return "return-value-2";
                  },
                  get done() {
                    log4.push({
                      name: "get return done (2)",
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



let callCount4 = 0;

async function* gen4() {
  callCount4 += 1;
  log4.push({ name: "before yield*" });
    yield* obj4;

}


const log5: any[] = [];
const obj5: any = {
  [Symbol.iterator]() {
    var returnCount = 0;
    return {
      name: "syncIterator",
      get next() {
        log5.push({ name: "get next" });
        return function() {
          return {
            value: "next-value-1",
            done: false
          };
        };
      },
      get return() {
        log5.push({
          name: "get return",
          thisValue: this
        });
        return function() {
          log5.push({
            name: "call return",
            thisValue: this,
            args: [...arguments]
          });

          returnCount++;
          if (returnCount == 1) {
            return {
              name: "return-result-1",
              get value() {
                log5.push({
                  name: "get return value (1)",
                  thisValue: this
                });
                return "return-value-1";
              },
              get done() {
                log5.push({
                  name: "get return done (1)",
                  thisValue: this
                });
                return false;
              }
            };
          }

          return {
            name: "return-result-2",
            get value() {
              log5.push({
                name: "get return value (2)",
                thisValue: this
              });
              return "return-value-2";
            },
            get done() {
              log5.push({
                name: "get return done (2)",
                thisValue: this
              });
              return true;
            }
          };
        };
      }
    };
  }
};



let callCount5 = 0;

async function* gen5() {
  callCount5 += 1;
  log5.push({ name: "before yield*" });
    yield* obj5;

}

async function* inner6() { try { yield 1; yield 2; } finally { console.log("inner-fin"); } }
async function* gen6() { try { const r: any = yield* inner6(); console.log("after", r); } finally { console.log("outer-fin"); } }
const reason7 = { why: "value getter" };
const obj7: any = { [Symbol.asyncIterator]() { return { next() { return { done: true, get value() { throw reason7; } }; } }; } };
async function* gen7() { yield* obj7; console.log("not reached 7"); }
const obj8: any = { [Symbol.asyncIterator]() { return { next() { return { done: false, get value() { throw reason7; } }; } }; } };
async function* gen8() { yield* obj8; console.log("not reached 8"); }
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
  it = gen4();
  v = await it.next("next-arg-1"); dump("async-return s1", log4);
  v = await it.return("return-arg-1"); dump("async-return s2", log4); console.log("v2", v.value, v.done);
  v = await it.next(); dump("async-return s3", log4); console.log("v3", v.value, v.done);
  it = gen5();
  v = await it.next("next-arg-1"); dump("sync-return s1", log5);
  v = await it.return("return-arg-1"); dump("sync-return s2", log5); console.log("v2", v.value, v.done);
  it = gen6();
  console.log("n1", JSON.stringify(await it.next()));
  console.log("ret", JSON.stringify(await it.return(9)));
  console.log("n2", JSON.stringify(await it.next()));
  it = gen6();
  await it.next();
  console.log("thr", await it.throw(new Error("x")).then((r: any) => JSON.stringify(r), (e: any) => "rej:" + e.message));
  it = gen7();
  console.log("done-getter", await it.next().then(() => "fulfilled", (e: any) => "rej same:" + (e === reason7)));
  console.log("done-getter after", JSON.stringify(await it.next()));
  it = gen8();
  console.log("notdone-getter", await it.next().then(() => "fulfilled", (e: any) => "rej same:" + (e === reason7)));
  console.log("notdone-getter after", JSON.stringify(await it.next()));
}
main();
