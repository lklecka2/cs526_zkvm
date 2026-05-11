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

ZK_ARENA_BUFFER(group_storage, ZKVM_BENCH_PAGES * SLOT_BYTES);

static unsigned char *groups[ZKVM_BENCH_PAGES];

static uint32_t page_of(const unsigned char *ptr) {
  return ((uint32_t)(size_t)ptr) / PAGE_BYTES;
}

static uint32_t is_aligned_one_page(const unsigned char *ptr) {
  return (((uint32_t)(size_t)ptr) & (PAGE_BYTES - 1u)) == 0u &&
         page_of(ptr) == page_of(ptr + PAGE_BYTES - 1u);
}

int cmain(void) {
  uint32_t acc = 0x6d2b79f5u;

  for (uint32_t i = 0; i < ZKVM_BENCH_PAGES; i++) {
    groups[i] = group_storage + i * SLOT_BYTES;
    if (!is_aligned_one_page(groups[i])) {
      return -1;
    }
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

  acc ^= ZKVM_BENCH_PAGES << 16;
  acc ^= ((uint32_t)(size_t)groups[0]) & (PAGE_BYTES - 1u);
  return (int)acc;
}
