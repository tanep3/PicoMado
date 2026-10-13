#pragma once
#include <stddef.h>
#include <stdint.h>
#include <stdarg.h>
#include <stdio.h>
#ifndef PM_TRACE_ENABLE
#define PM_TRACE_ENABLE 0
#endif

typedef enum {
    PM_TRACE_BUTTON, PM_TRACE_FRAME_START, PM_TRACE_CRC_OK,
    PM_TRACE_RECEIPT_BEGIN, PM_TRACE_RECEIPT_END,
    PM_TRACE_SPI_BEGIN, PM_TRACE_SPI_END,
    PM_TRACE_RENDER_BEGIN, PM_TRACE_RENDER_END,
    PM_TRACE_TCP_WRITE, PM_TRACE_TCP_OUTPUT, PM_TRACE_SLOW_POLL,
    PM_TRACE_SLOW_LOOP
} pm_trace_kind;
#if PM_TRACE_ENABLE
uint64_t pm_trace_clock(void);
void pm_trace_event(pm_trace_kind kind,uint64_t frame,int32_t a,uint32_t b);
void pm_trace_slow(pm_trace_kind kind,uint64_t started,uint32_t where);
void pm_trace_frame_context(uint64_t frame);
uint64_t pm_trace_current_frame(void);
int pm_logf(const char *format,...) __attribute__((format(printf,1,2)));
void pm_trace_flush(void);
uint32_t pm_trace_dropped(void);
size_t pm_trace_usb_write(const char *bytes,size_t length);
#else
static inline uint64_t pm_trace_clock(void){return 0;}
static inline void pm_trace_event(pm_trace_kind k,uint64_t f,int32_t a,uint32_t b){(void)k;(void)f;(void)a;(void)b;}
static inline void pm_trace_slow(pm_trace_kind k,uint64_t s,uint32_t w){(void)k;(void)s;(void)w;}
static inline void pm_trace_frame_context(uint64_t f){(void)f;}
static inline uint64_t pm_trace_current_frame(void){return 0;}
static inline int pm_logf(const char *format,...){va_list args;va_start(args,format);int n=vprintf(format,args);va_end(args);return n;}
static inline void pm_trace_flush(void){}
static inline uint32_t pm_trace_dropped(void){return 0;}
#endif
