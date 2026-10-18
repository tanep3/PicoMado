#pragma once
#include "endpoint_policy.h"
#include "eex_exchange.h"
typedef struct { bool ready; uint32_t ipv4,gateway,netmask; } pm_endpoint_network;
typedef struct {
    void *context;
    pm_endpoint_network (*network)(void *);
    bool (*open)(void *,uint32_t local_ipv4);
    void (*close)(void *);
    size_t (*receive)(void *,uint8_t *,size_t,pm_eex_peer *);
    bool (*send)(void *,const uint8_t *,size_t,pm_eex_peer);
    uint32_t (*random)(void *);
} pm_endpoint_io;
typedef struct {
    pm_endpoint_io io;
    pm_endpoint_network observed;
    pm_endpoint_snapshot network;
    pm_endpoint_policy policy;
    pm_endpoint_request request;
    pm_endpoint_cache cache;
    pm_eex_exchange exchange;
    bool enabled,opened,probe_pending,failed,changed;
    uint64_t next_snapshot,request_generation;
    uint32_t sent,received,send_errors;
    uint16_t discovery_port;
    uint8_t rx[PM_EEX_MAX_DATAGRAM];
    char tx[PM_EEX_MAX_DATAGRAM+1];
    pm_eex_message incoming;
} pm_endpoint_client;
void pm_endpoint_client_init(pm_endpoint_client *,pm_endpoint_io);
void pm_endpoint_client_reset(pm_endpoint_client *);
bool pm_endpoint_client_begin(pm_endpoint_client *,const char *client,uint64_t now_ms);
void pm_endpoint_client_poll(pm_endpoint_client *,uint64_t now_ms);
bool pm_endpoint_client_take(pm_endpoint_client *,uint64_t now_ms,pm_endpoint *);
void pm_endpoint_client_result(pm_endpoint_client *,pm_eex_status,pm_eex_stage,uint64_t now_ms);
bool pm_endpoint_client_network_changed(pm_endpoint_client *);
// SDK adapter is separate from observation/selection/exchange logic.
pm_endpoint_io pm_endpoint_sdk_io(void);
