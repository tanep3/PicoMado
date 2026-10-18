#pragma once
#include "trace.h"

typedef enum { PM_RX_COMPLETE=1,PM_RX_RESET,PM_RX_REPLACED,PM_RX_PEER_CLOSED,
    PM_RX_TCP_ERROR,PM_RX_OVERFLOW,PM_RX_INVALID,PM_RX_ACTIVE,PM_RX_CONNECT,
    PM_RX_IDLE,PM_RX_SEND_ERROR,PM_RX_ALLOC_ERROR } pm_rx_reason;
enum { PM_RX_POOL=1,PM_RX_HEAP=2,PM_RX_TCP=4,PM_RX_SEG=8 };
typedef struct {
    uint32_t flags,bits,pool_err,pool_used,heap_err,heap_used;
    uint32_t tcp_memerr,tcp_drop,tcp_chkerr,tcp_err;
    uint32_t seg_err,seg_used;
} pm_rx_alloc;
typedef struct {
    uint32_t rx,app,credit,tcp_valid,wnd,ann,nxt,ooseq;
    pm_rx_alloc alloc;
} pm_rx_state;
#define PM_RX_RECORD_PARTS 22u
#define PM_RX_EVENT_PARTS 3u
#if PM_TRACE_STATS
typedef struct {
    uint64_t frame,session,start,end,gap_from,gap_to,progress_at,last_cb,last_consume;
    uint64_t rx_peak_at,credit_peak_at,first_failure;
    uint32_t connection,reason,error,usb,assembled;
    uint32_t gap_rx0,gap_credit0,gap_rx1,gap_credit1;
    uint32_t gap_anchor,gap_tail;
    uint32_t callbacks,bytes,free_calls,freed,early_calls,early_bytes;
    uint32_t consume_calls,consume_credit,consumed,takes,messages;
    uint32_t rx_peak,app_peak,credit_peak,pool_peak,heap_peak,seg_peak;
    uint32_t wnd_min,ann_min,ooseq_seen,failure_mask;
    pm_rx_state before,after;
} pm_rx_record;
void pm_rx_frame_begin(uint64_t frame,uint64_t session,const pm_rx_state *state);
void pm_rx_finish(pm_rx_reason reason,int error,const pm_rx_state *state);
void pm_rx_sample(const pm_rx_state *state);
bool pm_rx_sample_due(void);
void pm_rx_callback_begin(uint32_t bytes,const pm_rx_state *state);
void pm_rx_callback_end(uint32_t freed,uint32_t credit,const pm_rx_state *state);
void pm_rx_consume(uint32_t bytes,uint32_t credit,const pm_rx_state *state);
void pm_rx_take(bool message,const pm_rx_state *state);
void pm_rx_progress(uint32_t bytes);
void pm_rx_connection(const pm_rx_state *state);
void pm_rx_closed(const pm_rx_state *state);
void pm_rx_fault(pm_rx_reason reason,int error,const pm_rx_state *state);
void pm_rx_usb(bool connected);
void pm_rx_capture(uint64_t at);
bool pm_rx_emit(uint32_t epoch);
size_t pm_rx_ram_bytes(void);
bool pm_rx_snapshot_record(unsigned slot,pm_rx_record *out);
#else
static inline void pm_rx_progress(uint32_t bytes){(void)bytes;}
#endif
