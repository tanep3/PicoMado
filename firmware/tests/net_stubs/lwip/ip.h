#pragma once
#include "netif.h"
static bool fake_ipv6;
#define IP_IS_V4(p) ((void)(p),!fake_ipv6)
#define ip_route(src,dst) ((void)(src),(void)(dst),fake_route)
#define ip4_addr_netcmp(a,b,m) (((a)->addr&(m)->addr)==((b)->addr&(m)->addr))
#define ip4_addr_islinklocal(a) (false)
#define ip4_addr_isany_val(a) ((a).addr==0)
