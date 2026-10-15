#pragma once
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

typedef struct {
    const char *ssid, *password;
    const char *server_ip;
    bool socks;
    const char *proxy_ip, *proxy_user, *proxy_password;
    uint16_t proxy_port;
} pm_profile;

typedef enum { PM_NET_DOWN, PM_NET_TCP, PM_NET_SOCKS, PM_NET_WS, PM_NET_READY, PM_NET_FAILED } pm_net_stage;
bool pm_net_init(void);
void pm_net_begin(const pm_profile *profile);
void pm_net_poll(void);
void pm_net_close(void);
pm_net_stage pm_net_state(void);
bool pm_net_take(uint8_t *kind, char *json, size_t cap);
size_t pm_net_payload_length(void);
bool pm_net_send(uint8_t kind, const char *json);
