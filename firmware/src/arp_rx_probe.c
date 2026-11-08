#include "arp_rx_probe.h"
#include <string.h>
#ifndef PM_ARP_RX_TEST
#include "pico/cyw43_arch.h"
#include "pico/stdlib.h"
#endif
static pm_arp_rx_row rows[PM_ARP_RX_CAP];
static uint32_t count,dropped;
static uint8_t server[4];
void pm_arp_rx_server(const uint8_t ip[4]){memcpy(server,ip,4);}
bool pm_arp_rx_get(unsigned i,pm_arp_rx_row *row,uint32_t *n,uint32_t *drop){*n=count;*drop=dropped;if(i>=count)return false;*row=rows[i];return true;}
static bool nonzero(const uint8_t *a){return a[0]||a[1]||a[2]||a[3];}
static bool related(const uint8_t *a,const uint8_t *local){return (nonzero(server)&&!memcmp(a,server,4))||(nonzero(local)&&!memcmp(a,local,4));}
static void observe(int itf,size_t len,const uint8_t *b,const uint8_t local[4]){
 if(!b||len<42||b[12]!=8||b[13]!=6||b[14]!=0||b[15]!=1||b[16]!=8||b[17]!=0||b[18]!=6||b[19]!=4||b[20]!=0||(b[21]!=1&&b[21]!=2))return;
 if(!related(b+28,local)&&!related(b+38,local))return;
 if(count==PM_ARP_RX_CAP){if(dropped!=UINT32_MAX)++dropped;return;}
 pm_arp_rx_row *r=&rows[count++];r->at_us=time_us_64();r->itf=(uint8_t)itf;r->opcode=b[21];
 memcpy(r->eth_src,b+6,6);memcpy(r->eth_dst,b,6);memcpy(r->sender_mac,b+22,6);memcpy(r->sender_ip,b+28,4);memcpy(r->target_mac,b+32,6);memcpy(r->target_ip,b+38,4);
}
void __real_cyw43_cb_process_ethernet(void *,int,size_t,const uint8_t *);
void __wrap_cyw43_cb_process_ethernet(void *data,int itf,size_t len,const uint8_t *buf){
 if(data&&itf==CYW43_ITF_STA){cyw43_t *self=data;observe(itf,len,buf,(const uint8_t *)netif_ip4_addr(&self->netif[itf]));}
 __real_cyw43_cb_process_ethernet(data,itf,len,buf);
}
