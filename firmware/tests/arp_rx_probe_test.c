#include <assert.h>
#include <stdint.h>
#include <stddef.h>
#include <string.h>
#define PM_ARP_RX_TEST 1
#define CYW43_ITF_STA 0
typedef struct {uint8_t ip[4];} fake_netif;
typedef struct {fake_netif netif[2];} cyw43_t;
#define netif_ip4_addr(n) ((n)->ip)
static uint64_t time_us_64(void){return 123;}
#include "../src/arp_rx_probe.c"
static unsigned forwarded;static void *expected;static int expected_itf;static size_t expected_len;static const uint8_t *expected_buf;
void __real_cyw43_cb_process_ethernet(void *d,int i,size_t n,const uint8_t*b){assert(d==expected&&i==expected_itf&&n==expected_len&&b==expected_buf);forwarded++;}
static void call(void*d,int itf,size_t n,uint8_t*b){uint8_t copy[64];assert(n<=64);if(b)memcpy(copy,b,n);expected=d;expected_itf=itf;expected_len=n;expected_buf=b;unsigned before=forwarded;__wrap_cyw43_cb_process_ethernet(d,itf,n,b);assert(forwarded==before+1);if(b)assert(!memcmp(copy,b,n));}
int main(void){
 cyw43_t d={.netif={{{192,168,0,2}},{{0}}}};uint8_t b[64]={0};uint8_t ip[4]={192,168,0,120};pm_arp_rx_server(ip);
 b[12]=8;b[13]=6;b[15]=1;b[16]=8;b[18]=6;b[19]=4;b[21]=2;memcpy(b+28,ip,4);b[27]=0xf3;
 for(size_t n=0;n<42;n++){call(&d,0,n,b);}assert(count==0);
 call(NULL,0,42,b);call(&d,1,42,b);assert(count==0);
 b[13]=0;call(&d,0,42,b);assert(count==0);b[13]=6;
 b[21]=3;call(&d,0,42,b);assert(count==0);b[21]=2;
 call(&d,0,42,b);assert(count==1&&rows[0].sender_mac[5]==0xf3&&rows[0].at_us==123);
 memset(b+28,0,4);call(&d,0,42,b);assert(count==1);
 memcpy(b+38,d.netif[0].ip,4);b[21]=1;call(&d,0,42,b);assert(count==2&&rows[1].opcode==1);
 for(unsigned i=0;i<40;i++){call(&d,0,42,b);}assert(count==32&&dropped==10);dropped=UINT32_MAX;call(&d,0,42,b);assert(dropped==UINT32_MAX);
 pm_arp_rx_row row;uint32_t n,drop;assert(pm_arp_rx_get(0,&row,&n,&drop)&&row.sender_mac[5]==0xf3);assert(!pm_arp_rx_get(32,&row,&n,&drop));
}
