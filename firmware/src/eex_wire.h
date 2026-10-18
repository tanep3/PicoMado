#pragma once
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#define PM_EEX_MAX_DATAGRAM 512u
typedef enum { PM_EEX_D, PM_EEX_O, PM_EEX_A, PM_EEX_R, PM_EEX_K } pm_eex_type;
typedef enum { PM_EEX_OK, PM_EEX_FAIL } pm_eex_status;
typedef enum {
    PM_EEX_NEGOTIATED, PM_EEX_TARGET_CONNECTED, PM_EEX_TCP_FAILED,
    PM_EEX_AUTH_REQUIRED, PM_EEX_PROTOCOL_ERROR, PM_EEX_TARGET_REJECTED,
    PM_EEX_TIMEOUT
} pm_eex_stage;
typedef struct {
    pm_eex_type type;
    char request[33], client[33], offer[33], generation[33];
    bool source_host;
    uint32_t host; // canonical a<<24|b<<16|c<<8|d
    uint16_t port;
    uint8_t ttl, attempt;
    pm_eex_status status;
    pm_eex_stage stage;
    char ack;
} pm_eex_message;
bool pm_eex_parse(const uint8_t *wire,size_t length,pm_eex_message *out);
size_t pm_eex_encode(const pm_eex_message *message,char *out,size_t capacity);
bool pm_eex_result_valid(pm_eex_status status,pm_eex_stage stage);
