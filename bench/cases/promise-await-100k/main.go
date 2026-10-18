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

func resolved[T any](v T) *promise[T] {
	return &promise[T]{value: v, settled: true}
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
			m.awaiting = awaiter[int64]{p: resolved(m.i)}
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
