#pragma once
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include "rx_stats.h"

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
