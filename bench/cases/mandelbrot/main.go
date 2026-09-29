package main

import (
	"fmt"
	"os"
	"strconv"
)

func mandel(cr, ci float64, maxIter int) int {
	zr := 0.0
	zi := 0.0
	n := 0
	for n < maxIter {
		if zr*zr+zi*zi > 4 {
			return n
		}
		newZr := zr*zr - zi*zi + cr
		zi = 2*zr*zi + ci
		zr = newZr
		n++
	}
	return maxIter
}

func main() {
	sideArg, _ := strconv.ParseInt(os.Args[1], 10, 64)
	maxIterArg, _ := strconv.ParseInt(os.Args[2], 10, 64)
	side := int(sideArg)
	maxIter := int(maxIterArg)
	total := 0
	for i := 0; i < side; i++ {
		for j := 0; j < side; j++ {
			cr := float64(i)/100 - 1.5
			ci := float64(j)/100 - 1.0
			total += mandel(cr, ci, maxIter)
		}
	}
	fmt.Println(total)
}
