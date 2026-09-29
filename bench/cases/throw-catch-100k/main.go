package main

import (
	"fmt"
	"os"
	"strconv"
)

func trial(i int64) (result int64) {
	defer func() {
		if r := recover(); r != nil {
			result = r.(int64)
		}
	}()
	panic(i)
}

func main() {
	n, _ := strconv.ParseInt(os.Args[1], 10, 64)
	var total int64 = 0
	for i := int64(0); i < n; i++ {
		total = total + trial(i)
	}
	fmt.Println(total)
}
