package main

import (
	"fmt"
	"os"
	"strconv"
)

type Fail struct {
	code int64
}

var finals int64

func descend(d, v, every int64) int64 {
	if d == 0 {
		if v%every == 0 {
			panic(&Fail{v})
		}
		return v
	}
	defer func() {
		finals++
	}()
	return descend(d-1, v+1, every) + 1
}

func attempt(depth, v, every int64) (r int64) {
	defer func() {
		if e := recover(); e != nil {
			r = e.(*Fail).code
		}
	}()
	return descend(depth, v, every)
}

func main() {
	n, _ := strconv.ParseInt(os.Args[1], 10, 64)
	depth, _ := strconv.ParseInt(os.Args[2], 10, 64)
	every, _ := strconv.ParseInt(os.Args[3], 10, 64)

	var total int64
	for i := int64(0); i < n; i++ {
		total += attempt(depth, i, every)
	}
	fmt.Println(total, finals)
}
