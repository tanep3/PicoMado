#pragma once
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#ifndef PM_STREAM_DIAGNOSTICS
#define PM_STREAM_DIAGNOSTICS 0
#endif
typedef enum {PM_D_OFFER,PM_D_REPLACE,PM_D_BUSY,PM_D_ORDER,PM_D_CONTEXT,
    PM_D_RESET,PM_D_DRAW,PM_D_FRAME,PM_D_CLOSE,PM_D_ERROR,PM_D_SAMPLE,
    PM_D_INVALID,PM_D_COUNT} pm_diag_kind;
enum {PM_D_GAP,PM_D_ASSEMBLY,PM_D_SPI,PM_D_HIST_COUNT};
#if PM_STREAM_DIAGNOSTICS
void pm_diag_event(pm_diag_kind kind,uint64_t session,uint64_t frame,uint64_t sequence,uint32_t value);
void pm_diag_draw_begin(void);
void pm_diag_draw_done(uint64_t now);
void pm_diag_frame_begin(uint64_t session,uint64_t frame,uint64_t sequence);
void pm_diag_frame_done(void);
void pm_diag_abort(void);
void pm_diag_connection(bool opened);
// Network callback latches only; main drains under the existing SDK lock.
void pm_diag_net_error(uint32_t value);
void pm_diag_drain_error(void);
void pm_diag_progress(uint32_t used);
bool pm_diag_sample_due(uint64_t now);
void pm_diag_sample(uint32_t rx,uint32_t app,uint32_t credit,uint32_t wnd,uint32_t nxt);
void pm_diag_usb(bool connected);
bool pm_diag_emit(uint32_t epoch);
size_t pm_diag_ram_bytes(void);
uint64_t pm_diag_hist_count(unsigned which);
uint32_t pm_diag_hist_bin(unsigned which,unsigned bin);
uint32_t pm_diag_counter(pm_diag_kind kind);
#else
static inline void pm_diag_event(pm_diag_kind k,uint64_t s,uint64_t f,uint64_t q,uint32_t v){(void)k;(void)s;(void)f;(void)q;(void)v;}
static inline void pm_diag_draw_begin(void){}
static inline void pm_diag_draw_done(uint64_t n){(void)n;}
static inline void pm_diag_frame_begin(uint64_t s,uint64_t f,uint64_t q){(void)s;(void)f;(void)q;}
static inline void pm_diag_frame_done(void){}
static inline void pm_diag_abort(void){}
static inline void pm_diag_connection(bool b){(void)b;}
static inline void pm_diag_net_error(uint32_t v){(void)v;}
static inline void pm_diag_drain_error(void){}
static inline void pm_diag_progress(uint32_t u){(void)u;}
static inline bool pm_diag_sample_due(uint64_t n){(void)n;return false;}
static inline void pm_diag_sample(uint32_t r,uint32_t a,uint32_t c,uint32_t w,uint32_t n){(void)r;(void)a;(void)c;(void)w;(void)n;}
static inline void pm_diag_usb(bool c){(void)c;}
static inline bool pm_diag_emit(uint32_t e){(void)e;return false;}
static inline size_t pm_diag_ram_bytes(void){return 0;}
#endif
