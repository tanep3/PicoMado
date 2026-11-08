/* Actual installed SDK ARP implementation, deterministic memory/link IO. */
#include <assert.h>
#include <stdbool.h>
#include <stdlib.h>
#include <string.h>
static bool sdk_locked=true;
#define LWIP_ASSERT_CORE_LOCKED() assert(sdk_locked)
#include "core/ipv4/etharp.c"
struct netif *netif_default;
const struct eth_addr ethbroadcast={{255,255,255,255,255,255}},ethzero={{0}};
static unsigned sent;
struct netif *ip4_route(const ip4_addr_t *ip){(void)ip;return netif_default;}
struct pbuf *pbuf_alloc(pbuf_layer layer,u16_t len,pbuf_type type){
    (void)layer;(void)type;struct pbuf *p=calloc(1,sizeof *p+len);assert(p);
    p->payload=p+1;p->len=p->tot_len=len;p->ref=1;return p;
}
u8_t pbuf_free(struct pbuf *p){free(p);return 1;}
err_t ethernet_output(struct netif *n,struct pbuf *p,const struct eth_addr *src,const struct eth_addr *dst,u16_t type){
    (void)n;(void)p;(void)src;(void)dst;(void)type;sent++;return ERR_OK;
}
static struct eth_addr host={{0x2c,0xcf,0x67,0x11,0x72,0xf3}},router={{0x10,0x3c,0x59,0x36,0xfb,0x2d}};
static void expect(struct netif *n,ip4_addr_t *ip,struct eth_addr *expected){
    struct eth_addr *actual;const ip4_addr_t *resolved;
    assert(etharp_find_addr(n,ip,&actual,&resolved)>=0);
    assert(ip4_addr_cmp(ip,resolved) && !memcmp(actual,expected,sizeof *actual));
}
int main(void){
    struct netif n={.hwaddr_len=6};ip4_addr_t peer;
    IP4_ADDR(ip_2_ip4(&n.ip_addr),192,168,0,44);IP4_ADDR(ip_2_ip4(&n.netmask),255,255,255,0);
    IP4_ADDR(&peer,192,168,0,120);netif_default=&n;
    assert(etharp_update_arp_entry(&n,&peer,&router,ETHARP_FLAG_TRY_HARD)==ERR_OK);expect(&n,&peer,&router);
    assert(etharp_add_static_entry(&peer,&host)==ERR_OK);expect(&n,&peer,&host);
    assert(etharp_update_arp_entry(&n,&peer,&router,ETHARP_FLAG_TRY_HARD)==ERR_VAL);expect(&n,&peer,&host);
    for(unsigned i=0;i<ARP_MAXAGE+2;i++)etharp_tmr();
    expect(&n,&peer,&host);assert(!sent);
    assert(etharp_remove_static_entry(&peer)==ERR_OK);assert(etharp_remove_static_entry(&peer)==ERR_MEM);
    assert(etharp_update_arp_entry(&n,&peer,&router,ETHARP_FLAG_TRY_HARD)==ERR_OK);
    assert(etharp_remove_static_entry(&peer)==ERR_ARG);expect(&n,&peer,&router);
    assert(etharp_add_static_entry(&peer,&host)==ERR_OK);etharp_cleanup_netif(&n);
    assert(etharp_remove_static_entry(&peer)==ERR_MEM);
    netif_default=NULL;assert(etharp_add_static_entry(&peer,&host)==ERR_RTE);
    puts("PASS installed SDK ARP: dynamic->static, conflicting update rejected, no aging, remove, netif cleanup and no-route");
}
