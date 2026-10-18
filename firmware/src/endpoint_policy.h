#pragma once
#include <stdbool.h>
#include <stdint.h>

// SDK-independent values. IPv4 is canonical a<<24|b<<16|c<<8|d, not lwIP byte order.
typedef struct { uint32_t ipv4; uint16_t port; } pm_endpoint;
typedef enum { PM_ENDPOINT_FIXED, PM_ENDPOINT_GATEWAY, PM_ENDPOINT_EXCHANGE } pm_endpoint_mode;
typedef struct {
    pm_endpoint_mode mode;
    uint32_t fixed_ipv4;
    uint16_t default_port;
    uint64_t revision;
} pm_endpoint_policy;
typedef struct {
    bool ipv4_ready;
    uint32_t local_ipv4, gateway_ipv4;
    uint64_t wifi_generation;
} pm_endpoint_snapshot;

// Internal generations only: no assumption about EEX1 nonce encoding or widths.
typedef struct { uint64_t wifi_generation, policy_revision, request_generation; } pm_endpoint_request;
typedef struct { bool valid; pm_endpoint endpoint; pm_endpoint_request request; } pm_endpoint_cache;
typedef enum {
    PM_ENDPOINT_READY, PM_ENDPOINT_WAIT_IP, PM_ENDPOINT_WAIT_GATEWAY,
    PM_ENDPOINT_WAIT_OFFER, PM_ENDPOINT_INVALID_ADDRESS,
    PM_ENDPOINT_INVALID_PORT, PM_ENDPOINT_INVALID_MODE
} pm_endpoint_status;
typedef struct {
    pm_endpoint_status status;
    pm_endpoint endpoint;
    pm_endpoint_mode source;
    uint64_t wifi_generation, policy_revision;
} pm_endpoint_selection;
typedef struct { uint64_t wifi_generation, policy_revision, attempt; } pm_endpoint_attempt;

bool pm_endpoint_ipv4_unicast(uint32_t ipv4);
pm_endpoint_selection pm_endpoint_select(const pm_endpoint_snapshot *network,
    const pm_endpoint_policy *policy, const pm_endpoint_cache *cache);
// Pure state reduction: only writes next after every guard passes. No IO or persistence.
// accepting is false while a candidate is in use; unsolicited offers cannot replace it.
bool pm_endpoint_accept_offer(const pm_endpoint_snapshot *network,
    const pm_endpoint_policy *policy, const pm_endpoint_request *current,
    const pm_endpoint_request *reply, pm_endpoint offered, bool accepting,
    pm_endpoint_cache *next);
bool pm_endpoint_attempt_current(pm_endpoint_attempt result, pm_endpoint_attempt current);
