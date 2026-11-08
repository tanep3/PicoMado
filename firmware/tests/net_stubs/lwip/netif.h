#pragma once
#include <stdbool.h>
#include "tcp.h"
struct netif;
typedef err_t (*netif_linkoutput_fn)(struct netif *,struct pbuf *);
struct netif {netif_linkoutput_fn linkoutput;ip4_addr_t ip,mask,gw;uint8_t num;bool up,link_up;};
extern struct netif *netif_default;

#define NETIF_NO_INDEX 0
static struct netif *fake_route;
static inline struct netif *netif_get_by_index(uint8_t n){(void)n;return fake_route;}
#define netif_get_index(n) ((n)->num+1)
#define netif_ip4_addr(n) (&(n)->ip)
#define netif_ip4_netmask(n) (&(n)->mask)
#define netif_ip4_gw(n) (&(n)->gw)
#define netif_is_up(n) ((n)->up)
#define netif_is_link_up(n) ((n)->link_up)
