package main

import (
	"fmt"
	"os"
	"strconv"
)

type future interface {
	poll(t *task) bool
}

type task struct {
	fut future
}

// single-threaded executor: every await resumption is one fifo microtask
var jobs []*task
var head int

func pollTask(t *task) {
	if t.fut == nil {
		return
	}
	if t.fut.poll(t) {
		t.fut = nil
	}
}

// runs the body synchronously up to its first await, as a js async call does
func spawn(f future) {
	pollTask(&task{fut: f})
}

func runMicrotasks() {
	for head < len(jobs) {
		t := jobs[head]
		jobs[head] = nil
		head++
		if head == len(jobs) {
			jobs = jobs[:0]
			head = 0
		}
		pollTask(t)
	}
}

type promise[T any] struct {
	value    T
	settled  bool
	awaiters []*task
}

func pending[T any]() *promise[T] {
	return &promise[T]{}
}

func (p *promise[T]) resolve(v T) {
	p.value = v
	p.settled = true
	awaiters := p.awaiters
	p.awaiters = nil
	jobs = append(jobs, awaiters...)
}

// await registers a reaction and always yields once, even on a settled promise
type awaiter[T any] struct {
	p          *promise[T]
	registered bool
}

func (a *awaiter[T]) poll(t *task) (T, bool) {
	if !a.registered {
		a.registered = true
		if a.p.settled {
			jobs = append(jobs, t)
		} else {
			a.p.awaiters = append(a.p.awaiters, t)
		}
		var zero T
		return zero, false
	}
	return a.p.value, a.p.settled
}

// async function double(v): the body never awaits, so its state machine
// settles the result promise on the first, synchronous poll
type doubleBody struct {
	v   int64
	ret *promise[int64]
}

func (d *doubleBody) poll(t *task) bool {
	d.ret.resolve(d.v + d.v)
	return true
}

func double(v int64) *promise[int64] {
	ret := pending[int64]()
	spawn(&doubleBody{v: v, ret: ret})
	return ret
}

// the module body as the state machine an async block lowers to
type mainBody struct {
	n, i, total int64
	awaiting    awaiter[int64]
	isAwaiting  bool
}

func (m *mainBody) poll(t *task) bool {
	for {
		if m.isAwaiting {
			v, ok := m.awaiting.poll(t)
			if !ok {
				return false
			}
			m.isAwaiting = false
			m.total = m.total + v
			m.i = m.i + 1
		}
		if m.i < m.n {
			m.awaiting = awaiter[int64]{p: double(m.i)}
			m.isAwaiting = true
			continue
		}
		fmt.Println(m.total)
		return true
	}
}

func main() {
	n, _ := strconv.ParseInt(os.Args[1], 10, 64)
	spawn(&mainBody{n: n})
	runMicrotasks()
}
