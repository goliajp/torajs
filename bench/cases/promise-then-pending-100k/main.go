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

// one fifo microtask: resume an awaiting task, or run a promise reaction
type job struct {
	task *task
	run  func()
}

// single-threaded executor over a js-style microtask queue
var jobs []job
var head int

func enqueue(j job) {
	jobs = append(jobs, j)
}

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
		j := jobs[head]
		jobs[head] = job{}
		head++
		if head == len(jobs) {
			jobs = jobs[:0]
			head = 0
		}
		if j.task != nil {
			pollTask(j.task)
		} else {
			j.run()
		}
	}
}

type reaction[T any] struct {
	then  func(T)
	await *task
}

type promise[T any] struct {
	value     T
	settled   bool
	reactions []reaction[T]
}

// a reaction on a settled promise becomes one queued microtask
func fire[T any](r reaction[T], v T) {
	if r.then != nil {
		f := r.then
		enqueue(job{run: func() { f(v) }})
	} else {
		enqueue(job{task: r.await})
	}
}

func pending[T any]() *promise[T] {
	return &promise[T]{}
}

func (p *promise[T]) resolve(v T) {
	p.value = v
	p.settled = true
	reactions := p.reactions
	p.reactions = nil
	for _, r := range reactions {
		fire(r, v)
	}
}

func (p *promise[T]) react(r reaction[T]) {
	if p.settled {
		fire(r, p.value)
	} else {
		p.reactions = append(p.reactions, r)
	}
}

func then[T, U any](p *promise[T], f func(T) U) *promise[U] {
	next := pending[U]()
	p.react(reaction[T]{then: func(v T) { next.resolve(f(v)) }})
	return next
}

// await registers a reaction and always yields once, even on a settled promise
type awaiter[T any] struct {
	p          *promise[T]
	registered bool
}

func (a *awaiter[T]) poll(t *task) (T, bool) {
	if !a.registered {
		a.registered = true
		a.p.react(reaction[T]{await: t})
		var zero T
		return zero, false
	}
	return a.p.value, a.p.settled
}

func add1(v int64) int64 {
	return v + 1
}

// async function later(v) { await null; return v } as its state
// machine: the first poll queues its own resumption and suspends, the
// second settles the result promise
type laterBody struct {
	v       int64
	yielded bool
	ret     *promise[int64]
}

func (s *laterBody) poll(t *task) bool {
	if !s.yielded {
		s.yielded = true
		enqueue(job{task: t})
		return false
	}
	s.ret.resolve(s.v)
	return true
}

func later(v int64) *promise[int64] {
	ret := pending[int64]()
	spawn(&laterBody{v: v, ret: ret})
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
			p := then(later(m.i), add1)
			m.awaiting = awaiter[int64]{p: p}
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
