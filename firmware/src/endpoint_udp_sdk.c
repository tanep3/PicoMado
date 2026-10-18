#include "endpoint_client.h"
#include "pico/cyw43_arch.h"
#include "pico/rand.h"
#include "lwip/udp.h"
#include "lwip/pbuf.h"
#include "lwip/netif.h"
#include "lwip/def.h"
#include <string.h>
// Two fixed incoming slots. No parser, logging or wait in the background callback.
typedef struct { uint8_t wire[PM_EEX_MAX_DATAGRAM]; uint16_t length; pm_eex_peer from; } inbox;
static struct { struct udp_pcb *pcb; inbox queue[2]; unsigned head,used; uint32_t dropped; } udp;
static uint32_t canonical(const ip_addr_t *a){return lwip_ntohl(ip4_addr_get_u32(ip_2_ip4(a)));}
static ip_addr_t address(uint32_t canonical_ip){
    ip_addr_t a;ip_addr_set_ip4_u32(&a,lwip_htonl(canonical_ip));return a;
}
static void received(void *arg,struct udp_pcb *pcb,struct pbuf *p,const ip_addr_t *from,u16_t port){
    (void)arg;(void)pcb;
    if(p&&IP_IS_V4(from)&&port&&p->tot_len&&p->tot_len<=PM_EEX_MAX_DATAGRAM&&udp.used<2){
        inbox *q=&udp.queue[(udp.head+udp.used)%2];
        q->length=p->tot_len;q->from=(pm_eex_peer){canonical(from),port};
        if(pbuf_copy_partial(p,q->wire,q->length,0)==q->length)udp.used++;else udp.dropped++;
    }else udp.dropped++;
    if(p)pbuf_free(p);
}
static pm_endpoint_network network(void *unused){
    (void)unused;pm_endpoint_network out;
    cyw43_arch_lwip_begin();
    struct netif *n=&cyw43_state.netif[CYW43_ITF_STA];
    out.ready=cyw43_tcpip_link_status(&cyw43_state,CYW43_ITF_STA)==CYW43_LINK_UP;
    out.ipv4=lwip_ntohl(ip4_addr_get_u32(netif_ip4_addr(n)));
    out.gateway=lwip_ntohl(ip4_addr_get_u32(netif_ip4_gw(n)));
    out.netmask=lwip_ntohl(ip4_addr_get_u32(netif_ip4_netmask(n)));
    cyw43_arch_lwip_end();return out;
}
static bool open_udp(void *unused,uint32_t local){
    (void)unused;cyw43_arch_lwip_begin();
    udp.pcb=udp_new_ip_type(IPADDR_TYPE_V4);udp.head=udp.used=0;
    if(udp.pcb){
        ip_addr_t local_addr=address(local);
        if(udp_bind(udp.pcb,&local_addr,0)!=ERR_OK){udp_remove(udp.pcb);udp.pcb=NULL;}
        else {udp_bind_netif(udp.pcb,&cyw43_state.netif[CYW43_ITF_STA]);
            udp.pcb->so_options|=SOF_BROADCAST;udp_recv(udp.pcb,received,NULL);}
    }
    bool ready=udp.pcb!=NULL;cyw43_arch_lwip_end();return ready;
}
static void close_udp(void *unused){
    (void)unused;cyw43_arch_lwip_begin();
    if(udp.pcb){udp_recv(udp.pcb,NULL,NULL);udp_remove(udp.pcb);udp.pcb=NULL;}
    udp.head=udp.used=0;cyw43_arch_lwip_end();
}
static size_t receive_udp(void *unused,uint8_t *out,size_t capacity,pm_eex_peer *from){
    (void)unused;size_t n=0;cyw43_arch_lwip_begin();
    if(udp.used){
        inbox *q=&udp.queue[udp.head];
        if(q->length<=capacity){n=q->length;memcpy(out,q->wire,n);*from=q->from;}
        udp.head=(udp.head+1)%2;udp.used--;
    }
    cyw43_arch_lwip_end();return n;
}
static bool send_udp(void *unused,const uint8_t *wire,size_t length,pm_eex_peer to){
    (void)unused;bool ok=false;cyw43_arch_lwip_begin();
    if(udp.pcb&&length<=PM_EEX_MAX_DATAGRAM){
        struct pbuf *p=pbuf_alloc(PBUF_TRANSPORT,(u16_t)length,PBUF_RAM);
        if(p){
            ip_addr_t destination=address(to.ipv4);
            if(pbuf_take(p,wire,(u16_t)length)==ERR_OK)
                ok=udp_sendto_if_src(udp.pcb,p,&destination,to.port,
                    &cyw43_state.netif[CYW43_ITF_STA],netif_ip_addr4(&cyw43_state.netif[CYW43_ITF_STA]))==ERR_OK;
            pbuf_free(p);
        }
    }
    cyw43_arch_lwip_end();return ok;
}
static uint32_t random32(void *unused){(void)unused;return get_rand_32();}
pm_endpoint_io pm_endpoint_sdk_io(void){
    return (pm_endpoint_io){NULL,network,open_udp,close_udp,receive_udp,send_udp,random32};
}
