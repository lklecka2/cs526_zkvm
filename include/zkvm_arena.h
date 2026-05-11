#ifndef NEW_ZKVM_ARENA_H
#define NEW_ZKVM_ARENA_H

#define ZK_ARENA_PAGE_BYTES 1024u
#define ZK_ARENA_DEFAULT_ALIGN 16u

typedef struct {
  unsigned char *base;
  unsigned int capacity;
  unsigned int offset;
  unsigned int high_water;
  unsigned int alloc_count;
  unsigned int failed_alloc_count;
} zk_arena_t;

#define ZK_ARENA_BUFFER(name, bytes) \
  static unsigned char name[(bytes)] __attribute__((aligned(ZK_ARENA_PAGE_BYTES)))

#define ZKVM_HOT_SLOTS(array, logical_count, stride_words) \
  do {                                                     \
    (void)(array);                                        \
    (void)(logical_count);                                \
    (void)(stride_words);                                 \
  } while (0)

#define ZKVM_REGION_SCOPE(name, bytes)       \
  ZK_ARENA_BUFFER(name##_storage, (bytes));  \
  zk_arena_t name;                           \
  zk_arena_init(&(name), name##_storage, sizeof(name##_storage))

#define ZKVM_REGION_ALLOC(name, type, count) \
  ((type *)zk_arena_alloc(&(name), (unsigned int)(sizeof(type) * (count)), ZK_ARENA_DEFAULT_ALIGN))

static unsigned int zk_arena_is_power_of_two(unsigned int value) {
  return value != 0 && (value & (value - 1u)) == 0;
}

static unsigned int zk_arena_align_up(unsigned int value, unsigned int align) {
  return (value + align - 1u) & ~(align - 1u);
}

static void zk_arena_init(zk_arena_t *arena, void *buffer, unsigned int capacity) {
  arena->base = (unsigned char *)buffer;
  arena->capacity = capacity;
  arena->offset = 0;
  arena->high_water = 0;
  arena->alloc_count = 0;
  arena->failed_alloc_count = 0;
}

static void *zk_arena_alloc(zk_arena_t *arena, unsigned int size, unsigned int align) {
  if (!zk_arena_is_power_of_two(align)) {
    arena->failed_alloc_count += 1u;
    return NULL;
  }

  unsigned int start = zk_arena_align_up(arena->offset, align);
  if (start > arena->capacity || size > arena->capacity - start) {
    arena->failed_alloc_count += 1u;
    return NULL;
  }

  arena->offset = start + size;
  if (arena->offset > arena->high_water) {
    arena->high_water = arena->offset;
  }
  arena->alloc_count += 1u;
  return arena->base + start;
}

static void zk_arena_reset(zk_arena_t *arena) {
  arena->offset = 0;
}

static unsigned int zk_arena_used(const zk_arena_t *arena) {
  return arena->offset;
}

static unsigned int zk_arena_high_water(const zk_arena_t *arena) {
  return arena->high_water;
}

static unsigned int zk_arena_pages_used(const zk_arena_t *arena) {
  return (arena->offset + ZK_ARENA_PAGE_BYTES - 1u) / ZK_ARENA_PAGE_BYTES;
}

static unsigned int zk_arena_high_water_pages(const zk_arena_t *arena) {
  return (arena->high_water + ZK_ARENA_PAGE_BYTES - 1u) / ZK_ARENA_PAGE_BYTES;
}

#endif
