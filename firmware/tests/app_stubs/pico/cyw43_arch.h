#pragma once
#include <stdbool.h>
#include <stdint.h>
#include <stddef.h>
typedef struct { unsigned unused; } cyw43_t;
extern cyw43_t cyw43_state;
int cyw43_arch_init(void);
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

void cyw43_arch_lwip_begin(void);
void cyw43_arch_lwip_end(void);

#define CYW43_NONE_PM 0x10u
#define CYW43_NO_POWERSAVE_MODE 0u
int cyw43_wifi_pm(cyw43_t *state,uint32_t value);
int cyw43_wifi_get_pm(cyw43_t *state,uint32_t *value);

int cyw43_ioctl(cyw43_t *state,uint32_t command,size_t length,uint8_t *buffer,uint32_t interface);
