#pragma once
#include <stdbool.h>
#include <stdint.h>
#ifndef PM_EVENT_STREAM
#define PM_EVENT_STREAM 0
#endif
/* Stable boundary vocabulary, shared with the host. No application decisions. */
enum { PM_OB_INPUT=1,PM_OB_API,PM_OB_NETWORK,PM_OB_FRAME,PM_OB_DISPLAY,PM_OB_STATE,PM_OB_EFFECT };
enum { PM_OB_RECEIVED=1,PM_OB_DISPATCHED,PM_OB_PENDING,PM_OB_COMPLETED,PM_OB_REJECTED,PM_OB_ERROR,PM_OB_CANCELLED,PM_OB_COMMITTED };
typedef struct {
    uint64_t boot,sequence,dropped,first_lost,last_lost;
    unsigned queued;
    bool enabled,saturated;
} pm_observation_status;
#if PM_EVENT_STREAM
void pm_observation_init(uint64_t boot);
void pm_observation_enable(bool enabled);
pm_observation_status pm_observation_read(void);
uint64_t pm_observe(uint8_t component,uint16_t event,uint8_t phase,
    uint64_t operation,uint64_t parent,uint64_t session,uint64_t frame,uint32_t a,uint32_t b);
#else
static inline void pm_observation_init(uint64_t b){(void)b;}
static inline void pm_observation_enable(bool e){(void)e;}
static inline pm_observation_status pm_observation_read(void){return (pm_observation_status){0};}
static inline uint64_t pm_observe(uint8_t c,uint16_t e,uint8_t p,uint64_t o,uint64_t r,uint64_t s,uint64_t f,uint32_t a,uint32_t b){(void)c;(void)e;(void)p;(void)o;(void)r;(void)s;(void)f;(void)a;(void)b;return 0;}
#endif
