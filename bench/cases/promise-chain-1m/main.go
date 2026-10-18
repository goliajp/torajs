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

func resolved[T any](v T) *promise[T] {
	return &promise[T]{value: v, settled: true}
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

// the module body as the state machine an async block lowers to
type mainBody struct {
	length     int32
	awaiting   awaiter[int64]
	isAwaiting bool
}

func (m *mainBody) poll(t *task) bool {
	for {
		if m.isAwaiting {
			v, ok := m.awaiting.poll(t)
			if !ok {
				return false
			}
			fmt.Println(v)
			return true
		}
		p := resolved[int64](0)
		for i := int32(0); i < m.length; i++ {
			p = then(p, add1)
		}
		m.awaiting = awaiter[int64]{p: p}
		m.isAwaiting = true
	}
}

func main() {
	length, _ := strconv.ParseInt(os.Args[1], 10, 32)
	spawn(&mainBody{length: int32(length)})
	runMicrotasks()
}
