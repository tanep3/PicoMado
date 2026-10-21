#pragma once
#include "tx_link_probe.h"
#ifndef PM_CYW43_INNER_PROBE
#define PM_CYW43_INNER_PROBE 0
#endif
#define PM_CYW_INNER_CAP 15u
#define PM_CYW_INNER_CRC_CAP 256u
#define PM_CYW_INNER_OFFSET 14u /* BDC4 + Ethernet; exclude SDPCM12 + padding2. */
#define PM_CYW_INNER_DEPTH 4u
typedef struct {uint32_t seq,end,ack;uint16_t ipid,length,type;uint8_t flags,valid;} pm_cyw_key;
enum {PM_CY_FULL=1u,PM_CY_WAIT=2u,PM_CY_RX_CRC=4u,PM_CY_BUS=8u,PM_CY_DONE=16u,PM_CY_CHANGED=32u,PM_CY_KEY_CHANGED=64u,PM_CY_NESTED=128u,PM_CY_SAT=256u,PM_CY_TIMEOUT=512u,PM_CY_INVALID=1024u,PM_CY_IO_ERROR=2048u,PM_CY_BAD_BUS_KEY=4096u,PM_CY_DEPTH_LOSS=8192u,PM_CY_PREFIX=16384u,PM_CY_RX_CHANGED=32768u};
typedef struct {
 uint64_t enter_us,bus_us,end_us;
 pm_cyw_key original,bus;
 uint32_t crc_before,crc_rx,crc_bus,bdc_before,bdc_bus;
 uint32_t wait_us,polls,io_calls,io_requested,io_errors,ticket;
 int32_t result;
 uint16_t flags,covered,domain,rx_data,rx_control,rx_event,rx_bad;
 int16_t rx_result;
 uint16_t rx_length;
 uint8_t flow_before,flow_bus,credit_before,credit_bus,sequence_before,sequence_bus,depth,nested;
 uint8_t dest_mac[6];
 uint16_t validity;
} pm_cyw_row;
typedef struct {
 uint64_t boot,run;
 uint32_t generation,count,dropped,incomplete,saturated,depth_lost,rejected,skipped,digest;
 uint32_t crc_calls,crc_bytes,crc_us,crc_max_us;
 bool active,changed;
} pm_cyw_status;
#if PM_CYW43_INNER_PROBE
/* ALL functions require the existing CYW43 SDK lock. Hooks never acquire it.
   Only the net/main adapter reads/resets. Records contain values, never buffers. */
extern const char pm_cyw43_inner_translation_unit[];
void pm_cyw_probe_sync(uint64_t boot,uint64_t run,uint32_t generation,bool active,bool connection_changed,uint64_t deadline,uint64_t frozen,const pm_tx_tuple *tuple);
uint32_t pm_cyw_probe_open(unsigned kind,size_t length,const uint8_t *buffer,size_t capacity,uint8_t flow,uint8_t credit,uint8_t sequence);
void pm_cyw_probe_copied(size_t expected,size_t actual);
void pm_cyw_probe_wait(uint32_t ticket);
void pm_cyw_probe_poll(uint32_t ticket,int result,size_t valid_length,const uint8_t *buffer,size_t capacity);
void pm_cyw_probe_read(size_t requested,int result);
void pm_cyw_probe_bus(uint32_t ticket,const uint8_t *buffer,size_t capacity,uint8_t flow,uint8_t credit,uint8_t sequence);
void pm_cyw_probe_finish(uint32_t ticket,int result,bool timed_out);
void pm_cyw_probe_reject(void);
void pm_cyw_probe_status(pm_cyw_status *out);
bool pm_cyw_probe_row(uint64_t boot,uint64_t run,unsigned index,pm_cyw_row *out);
uint32_t pm_cyw_crc32(const uint8_t *p,size_t n);
#else
static inline uint32_t pm_cyw_probe_open(unsigned k,size_t n,const uint8_t*p,size_t c,uint8_t f,uint8_t r,uint8_t s){(void)k;(void)n;(void)p;(void)c;(void)f;(void)r;(void)s;return 0;}
static inline void pm_cyw_probe_copied(size_t n,size_t a){(void)n;(void)a;}
static inline void pm_cyw_probe_wait(uint32_t t){(void)t;}
static inline void pm_cyw_probe_poll(uint32_t t,int r,size_t n,const uint8_t*p,size_t c){(void)t;(void)r;(void)n;(void)p;(void)c;}
static inline void pm_cyw_probe_read(size_t n,int r){(void)n;(void)r;}
static inline void pm_cyw_probe_bus(uint32_t t,const uint8_t*p,size_t c,uint8_t f,uint8_t r,uint8_t s){(void)t;(void)p;(void)c;(void)f;(void)r;(void)s;}
static inline void pm_cyw_probe_finish(uint32_t t,int r,bool b){(void)t;(void)r;(void)b;}
static inline void pm_cyw_probe_reject(void){}
#endif
