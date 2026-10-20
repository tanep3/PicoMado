#pragma once
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#ifndef PM_TX_LINK_PROBE
#define PM_TX_LINK_PROBE 0
#endif
#define PM_TX_LINK_CAP 32u
typedef struct { uint8_t local[4],peer[4];uint16_t local_port,peer_port; } pm_tx_tuple;
typedef struct {
    uint64_t time_us;
    uint32_t seq,end,ack,duration_us;
    uint16_t ipid,length;
    int8_t result;
    uint8_t nrtx,tcp_flags,pbuf_ref;
} pm_tx_row;
typedef struct {
    uint64_t boot,run,deadline_us;
    uint32_t generation,count,dropped,malformed;
    bool active,attached,generation_changed;
    pm_tx_tuple tuple;
    pm_tx_row rows[PM_TX_LINK_CAP];
} pm_tx_probe;
typedef bool (*pm_tx_reader)(const void *packet,unsigned offset,void *out,unsigned length);
typedef int (*pm_tx_forwarder)(void *context,const void *packet);
typedef uint64_t (*pm_tx_clock)(void);
int pm_tx_observe(pm_tx_probe *probe,pm_tx_reader read,const void *packet,pm_tx_forwarder forward,
                  void *context,pm_tx_clock clock,uint8_t nrtx,uint8_t refs);
uint32_t pm_tx_digest(const pm_tx_probe *probe);
