#pragma once
#include "tcp.h"
struct netif;
typedef err_t (*netif_linkoutput_fn)(struct netif *,struct pbuf *);
struct netif {netif_linkoutput_fn linkoutput;};
extern struct netif *netif_default;
