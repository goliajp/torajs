#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

typedef struct {
    int64_t *data;
    size_t len;
} I64Array;

/* a settled value: a number, or the array Promise.all produces */
typedef union {
    int64_t num;
    I64Array *arr;
} Value;

typedef void (*JobFn)(void *env, Value v);

typedef struct {
    JobFn fn;
    void *env;
    Value v;
} Job;

/* the fifo microtask queue, a power-of-two ring */
static Job *jobs;
static size_t jobs_cap, jobs_head, jobs_len;

static void enqueue(JobFn fn, void *env, Value v) {
    if (jobs_len == jobs_cap) {
        size_t cap = jobs_cap == 0 ? 64 : jobs_cap * 2;
        Job *next = malloc(cap * sizeof(Job));
        for (size_t k = 0; k < jobs_len; k++) {
            next[k] = jobs[(jobs_head + k) & (jobs_cap - 1)];
        }
        free(jobs);
        jobs = next;
        jobs_cap = cap;
        jobs_head = 0;
    }
    jobs[(jobs_head + jobs_len) & (jobs_cap - 1)] = (Job){fn, env, v};
    jobs_len++;
}

static void run_microtasks(void) {
    while (jobs_len > 0) {
        Job j = jobs[jobs_head];
        jobs_head = (jobs_head + 1) & (jobs_cap - 1);
        jobs_len--;
        j.fn(j.env, j.v);
    }
}

typedef struct Reaction {
    JobFn fn;
    void *env;
    struct Reaction *next;
} Reaction;

typedef struct {
    uint32_t rc;
    int fulfilled;
    Value value;
    Reaction *head, *tail;
} Promise;

static Promise *promise_new(void) {
    Promise *p = malloc(sizeof *p);
    *p = (Promise){1, 0, {0}, NULL, NULL};
    return p;
}

static Promise *promise_resolved(Value v) {
    Promise *p = malloc(sizeof *p);
    *p = (Promise){1, 1, v, NULL, NULL};
    return p;
}

static void promise_release(Promise *p) {
    if (--p->rc == 0) {
        free(p);
    }
}

static void promise_resolve(Promise *p, Value v) {
    p->fulfilled = 1;
    p->value = v;
    Reaction *r = p->head;
    p->head = p->tail = NULL;
    while (r) {
        Reaction *next = r->next;
        enqueue(r->fn, r->env, v);
        free(r);
        r = next;
    }
}

/* a settled promise queues the job now; a pending one keeps it in order */
static void promise_react(Promise *p, JobFn fn, void *env) {
    if (p->fulfilled) {
        enqueue(fn, env, p->value);
        return;
    }
    Reaction *r = malloc(sizeof *r);
    *r = (Reaction){fn, env, NULL};
    if (p->tail) {
        p->tail->next = r;
    } else {
        p->head = r;
    }
    p->tail = r;
}

typedef struct {
    Value (*f)(void *fenv, Value v);
    void *fenv;
    Promise *next;
} ThenEnv;

static void then_job(void *env, Value v) {
    ThenEnv *t = env;
    promise_resolve(t->next, t->f(t->fenv, v));
    promise_release(t->next);
    free(t);
}

/* the derived promise has two owners: the caller and the pending reaction */
static Promise *promise_then(Promise *p, Value (*f)(void *, Value), void *fenv) {
    Promise *next = promise_new();
    next->rc = 2;
    ThenEnv *t = malloc(sizeof *t);
    *t = (ThenEnv){f, fenv, next};
    promise_react(p, then_job, t);
    return next;
}

/* PerformPromiseAll: remaining starts at 1 so the result cannot settle
   before every element has been subscribed */
typedef struct {
    Promise *result;
    I64Array *values;
    size_t remaining;
} AllState;

typedef struct {
    AllState *st;
    size_t index;
} ElementEnv;

static void all_settle(AllState *st) {
    promise_resolve(st->result, (Value){.arr = st->values});
    promise_release(st->result);
    free(st);
}

/* the per-element resolve function; it returns undefined */
static Value all_element(void *fenv, Value v) {
    ElementEnv *e = fenv;
    AllState *st = e->st;
    st->values->data[e->index] = v.num;
    free(e);
    if (--st->remaining == 0) {
        all_settle(st);
    }
    return (Value){0};
}

static Promise *promise_all(Promise **ps, size_t n) {
    Promise *result = promise_new();
    result->rc = 2;
    AllState *st = malloc(sizeof *st);
    I64Array *values = malloc(sizeof *values);
    *values = (I64Array){malloc(n * sizeof(int64_t)), n};
    *st = (AllState){result, values, 1};
    for (size_t k = 0; k < n; k++) {
        st->remaining++;
        ElementEnv *e = malloc(sizeof *e);
        *e = (ElementEnv){st, k};
        promise_release(promise_then(ps[k], all_element, e));
    }
    if (--st->remaining == 0) {
        all_settle(st);
    }
    return result;
}

/* the module body as an async state machine; state 1 resumes after the await */
typedef struct {
    int state;
    Promise **arr;
    size_t len;
    Promise *awaiting;
} MainFrame;

static void main_step(void *env, Value v) {
    MainFrame *f = env;
    if (f->state == 0) {
        size_t cap = 0;
        f->arr = NULL;
        f->len = 0;
        for (int64_t i = 0; i < 1000; i++) {
            if (f->len == cap) {
                cap = cap == 0 ? 1 : cap * 2;
                f->arr = realloc(f->arr, cap * sizeof(Promise *));
            }
            f->arr[f->len++] = promise_resolved((Value){.num = i});
        }
        f->awaiting = promise_all(f->arr, f->len);
        f->state = 1;
        promise_react(f->awaiting, main_step, f);
        return;
    }
    promise_release(f->awaiting);
    I64Array *r = v.arr;
    int64_t total = 0;
    for (size_t i = 0; i < r->len; i++) {
        total = total + r->data[i];
    }
    printf("%" PRId64 "\n", total);
    for (size_t i = 0; i < f->len; i++) {
        promise_release(f->arr[i]);
    }
    free(f->arr);
    free(r->data);
    free(r);
    free(f);
}

int main(void) {
    MainFrame *f = malloc(sizeof *f);
    f->state = 0;
    main_step(f, (Value){0});
    run_microtasks();
    free(jobs);
    return 0;
}
