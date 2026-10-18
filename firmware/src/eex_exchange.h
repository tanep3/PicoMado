#pragma once
#include "eex_wire.h"
typedef enum { PM_EEX_DISCOVERING, PM_EEX_RECEIPT_WAIT, PM_EEX_PROBE_PENDING,
    PM_EEX_PROBING, PM_EEX_RESULT_WAIT, PM_EEX_DONE, PM_EEX_EXPIRED } pm_eex_phase;
typedef struct {
    uint32_t ipv4;
    uint16_t port;
} pm_eex_peer;
typedef struct {
    pm_eex_phase phase;
    uint64_t wifi_generation, discovery_deadline, offer_deadline, phase_deadline, next_send;
    uint32_t discovery_interval;
    pm_eex_message offer;
    pm_eex_peer reporter;
    uint8_t attempt;
    bool receipt_confirmed,result_confirmed,verified;
    pm_eex_status status;
    pm_eex_stage stage;
} pm_eex_exchange;
typedef struct {
    bool send,start_probe;
    pm_eex_message message;
} pm_eex_action;
// All functions are deterministic state transitions. No sockets, clocks, RNG or persistence.
bool pm_eex_begin(pm_eex_exchange *state,const char *request,const char *client,
    uint64_t wifi_generation,uint64_t now_ms,uint32_t wait_ms);
bool pm_eex_receive(pm_eex_exchange *state,const pm_eex_message *message,
    pm_eex_peer from,uint64_t now_ms);
pm_eex_action pm_eex_tick(pm_eex_exchange *state,uint64_t now_ms,unsigned jitter_per_mille);
bool pm_eex_probe_result(pm_eex_exchange *state,uint64_t wifi_generation,
    unsigned attempt,pm_eex_status status,pm_eex_stage stage,uint64_t now_ms);
