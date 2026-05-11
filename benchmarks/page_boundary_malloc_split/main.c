#include "../../include/zkvm.h"

#ifndef ZKVM_BENCH_PAGES
#define ZKVM_BENCH_PAGES 64
#endif

#ifndef ZKVM_BENCH_ROUNDS
#define ZKVM_BENCH_ROUNDS 64
#endif

#define PAGE_BYTES ZK_ARENA_PAGE_BYTES
#define LANES 8u
#define LANE_BYTES (PAGE_BYTES / LANES)
#define SLOT_BYTES (PAGE_BYTES * 2u)
#define MAX_WARMUPS 16u
#define SPLIT_THRESHOLD_NUM 95u
#define SPLIT_THRESHOLD_DEN 100u

static unsigned char *groups[ZKVM_BENCH_PAGES];
static void *warmups[MAX_WARMUPS];

static uint32_t page_of(const unsigned char *ptr) {
  return ((uint32_t)(size_t)ptr) / PAGE_BYTES;
}

static uint32_t is_split(const unsigned char *ptr) {
  return page_of(ptr) != page_of(ptr + PAGE_BYTES - 1u);
}

static uint32_t split_count(void) {
  uint32_t splits = 0;
  for (uint32_t i = 0; i < ZKVM_BENCH_PAGES; i++) {
    splits += is_split(groups[i]);
  }
  return splits;
}

int cmain(void) {
  uint32_t acc = 0x6d2b79f5u;

  for (uint32_t i = 0; i < 4u && i < MAX_WARMUPS; i++) {
    warmups[i] = malloc(LANE_BYTES);
    if (warmups[i] == NULL) {
      return -1;
    }
  }

  for (uint32_t i = 0; i < ZKVM_BENCH_PAGES; i++) {
    groups[i] = (unsigned char *)malloc(SLOT_BYTES);
    if (groups[i] == NULL) {
      return -2;
    }
  }

  uint32_t splits = split_count();
  if (splits * SPLIT_THRESHOLD_DEN < ZKVM_BENCH_PAGES * SPLIT_THRESHOLD_NUM) {
    return -3;
  }

  for (uint32_t round = 0; round < ZKVM_BENCH_ROUNDS; round++) {
    for (uint32_t pass = 0; pass < 2u; pass++) {
      uint32_t parity = 1u - pass;
      for (uint32_t group = parity; group < ZKVM_BENCH_PAGES; group += 2u) {
        unsigned char *base = groups[group];
        for (uint32_t lane = 0; lane < LANES; lane++) {
          uint32_t lane_base = lane * LANE_BYTES;
          base[lane_base] = (unsigned char)(acc + group + lane);
          base[lane_base + 31u] = (unsigned char)(round ^ group ^ lane);
          base[lane_base + 63u] = (unsigned char)(acc >> 7);
          base[lane_base + 127u] = (unsigned char)(acc >> 13);
          uint32_t value = base[lane_base];
          value ^= ((uint32_t)base[lane_base + 31u]) << 8;
          value ^= ((uint32_t)base[lane_base + 63u]) << 16;
          value ^= ((uint32_t)base[lane_base + 127u]) << 24;
          acc = (acc << 5) ^ (acc >> 3) ^ value ^ group ^ round;
          base[lane_base] = (unsigned char)acc;
          base[lane_base + 127u] = (unsigned char)(acc >> 11);
        }
      }
    }
  }

  acc ^= splits << 16;
  acc ^= ((uint32_t)(size_t)groups[0]) & (PAGE_BYTES - 1u);
  return (int)acc;
}
