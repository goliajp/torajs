use std::cell::RefCell;
use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};

// one fifo microtask: resume an awaiting task, or run a promise reaction
enum Job {
    Poll(usize),
    Run(Box<dyn FnOnce()>),
}

// single-threaded executor over a js-style microtask queue
struct Executor {
    jobs: VecDeque<Job>,
    tasks: Vec<Option<Pin<Box<dyn Future<Output = ()>>>>>,
    wakers: Vec<Waker>,
    free: Vec<usize>,
}

thread_local! {
    static EXEC: RefCell<Executor> = RefCell::new(Executor {
        jobs: VecDeque::new(),
        tasks: Vec::new(),
        wakers: Vec::new(),
        free: Vec::new(),
    });
}

fn enqueue(job: Job) {
    EXEC.with(|e| e.borrow_mut().jobs.push_back(job));
}

struct TaskWaker(usize);

impl Wake for TaskWaker {
    fn wake(self: Arc<Self>) {
        enqueue(Job::Poll(self.0));
    }
}

fn poll_task(id: usize) {
    let taken = EXEC.with(|e| {
        let mut e = e.borrow_mut();
        e.tasks[id].take().map(|fut| (fut, e.wakers[id].clone()))
    });
    let Some((mut fut, waker)) = taken else { return };
    if fut.as_mut().poll(&mut Context::from_waker(&waker)).is_pending() {
        EXEC.with(|e| e.borrow_mut().tasks[id] = Some(fut));
    } else {
        EXEC.with(|e| e.borrow_mut().free.push(id));
    }
}

// runs the body synchronously up to its first await, as a js async call does
fn spawn(fut: impl Future<Output = ()> + 'static) {
    let id = EXEC.with(|e| {
        let mut e = e.borrow_mut();
        match e.free.pop() {
            Some(id) => {
                e.tasks[id] = Some(Box::pin(fut));
                id
            }
            None => {
                let id = e.tasks.len();
                e.tasks.push(Some(Box::pin(fut)));
                e.wakers.push(Arc::new(TaskWaker(id)).into());
                id
            }
        }
    });
    poll_task(id);
}

fn run_microtasks() {
    loop {
        let job = EXEC.with(|e| e.borrow_mut().jobs.pop_front());
        match job {
            None => break,
            Some(Job::Poll(id)) => poll_task(id),
            Some(Job::Run(f)) => f(),
        }
    }
}

enum Reaction<T> {
    Then(Box<dyn FnOnce(T)>),
    Await(Waker),
}

struct Inner<T> {
    value: Option<T>,
    reactions: Vec<Reaction<T>>,
}

struct Promise<T>(Rc<RefCell<Inner<T>>>);

impl<T> Clone for Promise<T> {
    fn clone(&self) -> Self {
        Promise(self.0.clone())
    }
}

// a reaction on a settled promise becomes one queued microtask
fn fire<T: 'static>(r: Reaction<T>, v: T) {
    match r {
        Reaction::Then(f) => enqueue(Job::Run(Box::new(move || f(v)))),
        Reaction::Await(w) => w.wake(),
    }
}

impl<T: Clone + 'static> Promise<T> {
    fn pending() -> Self {
        Promise(Rc::new(RefCell::new(Inner {
            value: None,
            reactions: Vec::new(),
        })))
    }

    fn resolve(&self, v: T) {
        let reactions = {
            let mut inner = self.0.borrow_mut();
            inner.value = Some(v.clone());
            std::mem::take(&mut inner.reactions)
        };
        for r in reactions {
            fire(r, v.clone());
        }
    }

    fn react(&self, r: Reaction<T>) {
        let settled = self.0.borrow().value.clone();
        match settled {
            Some(v) => fire(r, v),
            None => self.0.borrow_mut().reactions.push(r),
        }
    }

    fn then<U: Clone + 'static>(&self, f: impl FnOnce(T) -> U + 'static) -> Promise<U> {
        let next = Promise::pending();
        let target = next.clone();
        self.react(Reaction::Then(Box::new(move |v| target.resolve(f(v)))));
        next
    }

    fn into_await(self) -> Await<T> {
        Await {
            p: self,
            registered: false,
        }
    }
}

// await registers a reaction and always yields once, even on a settled promise
struct Await<T> {
    p: Promise<T>,
    registered: bool,
}

impl<T: Clone + 'static> Future for Await<T> {
    type Output = T;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<T> {
        if !self.registered {
            self.registered = true;
            self.p.react(Reaction::Await(cx.waker().clone()));
            return Poll::Pending;
        }
        match &self.p.0.borrow().value {
            Some(v) => Poll::Ready(v.clone()),
            None => Poll::Pending,
        }
    }
}

fn add1(v: i64) -> i64 {
    v + 1
}

// async function later(v) { await null; return v } as its state machine:
// the first poll queues its own resumption and suspends, the second
// settles the result promise
struct LaterBody {
    v: i64,
    yielded: bool,
    ret: Promise<i64>,
}

impl Future for LaterBody {
    type Output = ();
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        if !self.yielded {
            self.yielded = true;
            cx.waker().wake_by_ref();
            return Poll::Pending;
        }
        self.ret.resolve(self.v);
        Poll::Ready(())
    }
}

fn later(v: i64) -> Promise<i64> {
    let ret = Promise::pending();
    spawn(LaterBody {
        v,
        yielded: false,
        ret: ret.clone(),
    });
    ret
}

// the module body as the state machine an async block lowers to
struct MainBody {
    n: i64,
    i: i64,
    total: i64,
    awaiting: Option<Await<i64>>,
}

impl Future for MainBody {
    type Output = ();
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let this = &mut *self;
        loop {
            if let Some(a) = this.awaiting.as_mut() {
                match Pin::new(a).poll(cx) {
                    Poll::Pending => return Poll::Pending,
                    Poll::Ready(v) => {
                        this.awaiting = None;
                        this.total = this.total + v;
                        this.i = this.i + 1;
                    }
                }
            }
            if this.i < this.n {
                let p = later(this.i).then(add1);
                this.awaiting = Some(p.into_await());
                continue;
            }
            println!("{}", this.total);
            return Poll::Ready(());
        }
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let n: i64 = args.next().unwrap().parse().unwrap();
    spawn(MainBody {
        n,
        i: 0,
        total: 0,
        awaiting: None,
    });
    run_microtasks();
}
