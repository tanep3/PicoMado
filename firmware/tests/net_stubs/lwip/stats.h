#pragma once
#include <stdint.h>
#define LWIP_STATS 1
#define MEMP_STATS 1
#define MEM_STATS 1
#define TCP_STATS 1
typedef uint16_t STAT_COUNTER;
struct stats_mem {STAT_COUNTER err;uint32_t used;};
struct stats_proto {STAT_COUNTER memerr,drop,chkerr,err;};
struct stats_ {struct stats_mem mem;struct stats_mem *memp[2];struct stats_proto tcp;};
extern struct stats_ lwip_stats;
