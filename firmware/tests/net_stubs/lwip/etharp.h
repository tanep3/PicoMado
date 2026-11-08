#pragma once
#include "netif.h"
struct eth_addr {uint8_t addr[6];};
static bool fake_arp_found;
static struct eth_addr fake_arp_mac;
static inline int etharp_find_addr(struct netif*n,const ip4_addr_t*ip,struct eth_addr**mac,const ip4_addr_t**out){(void)n;*out=ip;*mac=&fake_arp_mac;return fake_arp_found?0:-1;}
#if PM_HOME_STATIC_ARP_DIAGNOSTIC || PM_SERVER_LINK_HINT
err_t etharp_add_static_entry(const ip4_addr_t *,struct eth_addr *);
err_t etharp_remove_static_entry(const ip4_addr_t *);
#endif
