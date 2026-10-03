import sys


def survivor(people, k):
    r = 0
    i = 2
    while i <= people:
        r = (r + k) % i
        i += 1
    return r


limit = int(sys.argv[1])
people = int(sys.argv[2])
total = 0
k = 1
while k <= limit:
    total += survivor(people, k)
    k += 1
print(total)
