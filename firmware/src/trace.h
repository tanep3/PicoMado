#pragma once
#include <stddef.h>
#include <stdint.h>
#include <stdbool.h>
#include <stdarg.h>
#include <stdio.h>
#include "stream_diag.h"
#include "observation.h"
#ifndef PM_TRACE_ENABLE
#define PM_TRACE_ENABLE 0
#endif
#ifndef PM_TRACE_STATS
#define PM_TRACE_STATS 0
#endif
#ifndef PM_TRACE_VERBOSE
#define PM_TRACE_VERBOSE 1
#endif
#if PM_TRACE_STATS && !PM_TRACE_ENABLE
#error "RAM statistics require the bounded trace backend"
#endif

typedef enum {
    PM_TRACE_BUTTON, PM_TRACE_FRAME_START, PM_TRACE_CRC_OK,
    PM_TRACE_RECEIPT_BEGIN, PM_TRACE_RECEIPT_END,
    PM_TRACE_SPI_BEGIN, PM_TRACE_SPI_END,
    PM_TRACE_RENDER_BEGIN, PM_TRACE_RENDER_END,
    PM_TRACE_TCP_WRITE, PM_TRACE_TCP_OUTPUT, PM_TRACE_SLOW_POLL,
    PM_TRACE_SLOW_LOOP
} pm_trace_kind;
// Shared bounded USB line transport remains available without measurements.
bool pm_trace_try_line(const char *line,size_t length);
void pm_trace_flush(void);
size_t pm_trace_usb_write(const char *bytes,size_t length);
#if PM_TRACE_ENABLE
uint64_t pm_trace_clock(void);
void pm_trace_event(pm_trace_kind kind,uint64_t frame,int32_t a,uint32_t b);
void pm_trace_slow(pm_trace_kind kind,uint64_t started,uint32_t where);
void pm_trace_frame_context(uint64_t frame);
uint64_t pm_trace_current_frame(void);
int pm_logf(const char *format,...) __attribute__((format(printf,1,2)));
uint32_t pm_trace_dropped(void);
#else
static inline uint64_t pm_trace_clock(void){return 0;}
static inline void pm_trace_event(pm_trace_kind k,uint64_t f,int32_t a,uint32_t b){(void)k;(void)f;(void)a;(void)b;}
static inline void pm_trace_slow(pm_trace_kind k,uint64_t s,uint32_t w){(void)k;(void)s;(void)w;}
static inline void pm_trace_frame_context(uint64_t f){(void)f;}
static inline uint64_t pm_trace_current_frame(void){return 0;}
static inline int pm_logf(const char *format,...){(void)format;return 0;}
static inline uint32_t pm_trace_dropped(void){return 0;}
#endif

typedef enum {
    PM_STAT_LOOP, PM_STAT_LOOP_GAP, PM_STAT_SDK_WAIT, PM_STAT_NET_POLL,
    PM_STAT_RX_TAKE, PM_STAT_NET_SEND, PM_STAT_PROCESS, PM_STAT_DRAW,
    PM_STAT_FRAME_RX, PM_STAT_FLUSH, PM_STAT_USB_WRITE, PM_STAT_RX_CALLBACK,
    PM_STAT_COUNT
} pm_stat_kind;
typedef struct { uint32_t calls,max_us;
#if PM_STREAM_DIAGNOSTICS
    uint64_t total_us;
#endif
} pm_stat_sample;
static inline void pm_stats_accumulate(pm_stat_sample *s,uint64_t elapsed){
    if(s->calls<UINT32_MAX)s->calls++;
    uint32_t duration=elapsed>UINT32_MAX?UINT32_MAX:(uint32_t)elapsed;
    if(duration>s->max_us)s->max_us=duration;
#if PM_STREAM_DIAGNOSTICS
    s->total_us=UINT64_MAX-s->total_us<elapsed?UINT64_MAX:s->total_us+elapsed;
#endif
}
#if PM_TRACE_STATS
static inline uint64_t pm_stats_clock(void){return pm_trace_clock();}
void pm_stats_note(pm_stat_kind kind,uint64_t started);
void pm_stats_loop_begin(uint64_t started);
void pm_stats_frame_start(void);
void pm_stats_frame_done(void);
void pm_stats_usb_poll(bool connected);
pm_stat_sample pm_stats_read(bool connected,pm_stat_kind kind);
size_t pm_stats_ram_bytes(void);
#else
static inline uint64_t pm_stats_clock(void){return 0;}
static inline void pm_stats_note(pm_stat_kind k,uint64_t s){(void)k;(void)s;}
static inline void pm_stats_loop_begin(uint64_t s){(void)s;}
static inline void pm_stats_frame_start(void){}
static inline void pm_stats_frame_done(void){}
static inline void pm_stats_usb_poll(bool c){(void)c;}
#endif
