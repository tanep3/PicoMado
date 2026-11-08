#pragma once
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include "rx_stats.h"
#include "observation.h"
#include "home_static_arp.h"

typedef struct {
    const char *ssid, *password;
    const char *server_ip;
    bool socks;
    const char *proxy_ip, *proxy_user, *proxy_password;
    uint16_t proxy_port;
} pm_profile;

typedef enum { PM_NET_DOWN, PM_NET_TCP, PM_NET_SOCKS, PM_NET_WS, PM_NET_READY, PM_NET_DRAINING, PM_NET_DRAINED, PM_NET_FAILED } pm_net_stage;
#define PM_NET_EOF_DRAIN_US 1500000u
// EOF stages retain only this connection's copied bytes; never wait for more.
bool pm_net_init(void);
void pm_net_begin(const pm_profile *profile);
#if PM_HOME_STATIC_ARP_DIAGNOSTIC
void pm_net_begin_home_static_arp(const pm_profile *profile,pm_profile_id id);
#endif
void pm_net_poll(void);
// Check latched errors/EOF deadline under the SDK lock, without SDK polling,
// parsing or application writes. Failed/expired transports are closed.
// Use before starting a new synchronous network image draw.
pm_net_stage pm_net_checkpoint(void);
void pm_net_close(void);
pm_net_stage pm_net_state(void);
bool pm_net_take(uint8_t *kind, char *json, size_t cap);
size_t pm_net_payload_length(void);
bool pm_net_send(uint8_t kind, const char *json);
// No payload retained. WAIT means no COPY; caller owns bounded retry/lifetime.
// COPIED is enqueue acceptance, never delivery; output errors do not undo it.
typedef enum { PM_NET_TX_FAILED, PM_NET_TX_WAIT, PM_NET_TX_COPIED } pm_net_tx_result;
// Zero scope binds the first attempt; later attempts refuse another connection.
pm_net_tx_result pm_net_try_send(uint8_t kind,const char *json,uint64_t *connection);
// Main-loop owner calls only after checking its current session/stream scope.
void pm_net_send_expired(uint64_t connection);

#if PM_EVENT_STREAM
bool pm_net_send_observed(uint8_t kind,const char *json,uint64_t notify,uint64_t parent,uint64_t session);
void pm_net_observed_api(uint64_t operation,uint64_t frame);
#endif

bool pm_net_send_optional(uint8_t kind,const char *json);

#if PM_TRACE_STATS
void pm_net_rx_frame_start(uint64_t frame,uint64_t session);
void pm_net_rx_frame_end(pm_rx_reason reason,int error);
void pm_net_rx_snapshot(void);
#else
static inline void pm_net_rx_frame_start(uint64_t frame,uint64_t session){(void)frame;(void)session;}
static inline void pm_net_rx_frame_end(pm_rx_reason reason,int error){(void)reason;(void)error;}
static inline void pm_net_rx_snapshot(void){}
#endif

// Transport observations, independent of endpoint-discovery wire format.
typedef enum { PM_NET_FAIL_NONE, PM_NET_FAIL_TCP, PM_NET_FAIL_PROTOCOL, PM_NET_FAIL_TARGET, PM_NET_FAIL_EOF_TIMEOUT } pm_net_failure;
bool pm_net_proxy_target_connected(void);
pm_net_failure pm_net_last_failure(void);

#include "tx_link_probe.h"
typedef struct {uint64_t boot,run;uint32_t generation,count,dropped,malformed,digest;bool active,attached,generation_changed;} pm_tx_status;
#if PM_TX_LINK_PROBE
void pm_net_tx_status(pm_tx_status *status);
bool pm_net_tx_read(uint64_t boot,uint64_t run,unsigned index,pm_tx_row *row);
#else
static inline void pm_net_tx_status(pm_tx_status *s){*s=(pm_tx_status){0};}
static inline bool pm_net_tx_read(uint64_t b,uint64_t r,unsigned i,pm_tx_row *o){(void)b;(void)r;(void)i;(void)o;return false;}
#endif

#include "cyw43_inner_probe.h"
#if PM_CYW43_INNER_PROBE
void pm_net_cyw_status(pm_cyw_status *out);
bool pm_net_cyw_read(uint64_t boot,uint64_t run,unsigned index,pm_cyw_row *out);
#endif

// Snapshot only: TCP write acceptance is not peer acknowledgement.
typedef struct {
    uint64_t connection_id;
    bool valid,unsent,unacked,output_valid;
    uint8_t arp_status,arp_netif,peer_ipv4[4],next_hop[4],peer_mac[6];
    uint8_t state,nrtx,keep_cnt_sent;
    int16_t rtime,last_output_result;
    uint32_t snd_lbb,snd_nxt,lastack,snd_wnd,cwnd;
    uint32_t write_ok,write_failed,output_ok,output_failed;
} pm_net_tcp_status;
void pm_net_tcp_snapshot(pm_net_tcp_status *out);
