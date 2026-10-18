package main

import (
	"fmt"
	"os"
	"strconv"
)

const (
	stateStart = iota
	stateAfterYield
	stateDone
)

// function* range(start, end) as its generator state machine; the object
// lives on the heap like a js generator object
type Range struct {
	state int
	start int64
	end   int64
	i     int64
}

func (r *Range) next() (int64, bool) {
	switch r.state {
	case stateStart:
		r.i = r.start
	case stateAfterYield:
		r.i = r.i + 1
	case stateDone:
		return 0, false
	}
	if r.i < r.end {
		r.state = stateAfterYield
		return r.i, true
	}
	r.state = stateDone
	return 0, false
}

func newRange(start, end int64) *Range {
	return &Range{state: stateStart, start: start, end: end}
}

func main() {
	var total int64 = 0
	passes, _ := strconv.ParseInt(os.Args[1], 10, 32)
	end, _ := strconv.ParseInt(os.Args[2], 10, 64)
	for p := int64(0); p < passes; p++ {
		it := newRange(0, end)
		for v, ok := it.next(); ok; v, ok = it.next() {
			total = total + v
		}
	}
	fmt.Println(total)
}
