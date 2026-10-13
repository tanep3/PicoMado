#pragma once
#include <stdbool.h>
#include <stdint.h>
typedef struct { unsigned unused; } cyw43_t;
extern cyw43_t cyw43_state;
#define CYW43_ITF_STA 0
#define CYW43_AUTH_WPA2_AES_PSK 0x00400004u
#define CYW43_LINK_DOWN 0
#define CYW43_LINK_JOIN 1
#define CYW43_LINK_NOIP 2
#define CYW43_LINK_UP 3
#define CYW43_LINK_FAIL (-1)
#define CYW43_LINK_NONET (-2)
#define CYW43_LINK_BADAUTH (-3)
void cyw43_arch_enable_sta_mode(void);
void cyw43_arch_disable_sta_mode(void);
int cyw43_arch_wifi_connect_async(const char *ssid,const char *password,uint32_t auth);
void cyw43_arch_poll(void);
int cyw43_wifi_link_status(cyw43_t *state,int interface);
int cyw43_tcpip_link_status(cyw43_t *state,int interface);
