#pragma once
#include <stdint.h>
typedef struct { uint32_t addr; } ip4_addr_t;
typedef ip4_addr_t ip_addr_t;
#define IPADDR_TYPE_V4 0
#define ip_2_ip4(address) (address)
int ipaddr_aton(const char *text,ip_addr_t *address);
