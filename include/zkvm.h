#ifndef NEW_ZKVM_H
#define NEW_ZKVM_H

typedef unsigned int uint32_t;
typedef unsigned long long uint64_t;
typedef unsigned int size_t;

#ifndef NULL
#define NULL 0
#endif

void *malloc(size_t size);
void free(void *ptr);
void *memset(void *s, int c, size_t n);
void *memcpy(void *dest, const void *src, size_t n);

#include "zkvm_arena.h"

int cmain(void);

#endif
