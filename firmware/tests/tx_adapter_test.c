/* Production net.c wrapper and lifecycle, with SDK-lock model and callback thread. */
#pragma GCC diagnostic ignored "-Wmissing-field-initializers"
#define main existing_network_cases
#include "net_socks_test.c"
#undef main
#include <pthread.h>
struct netif *netif_default;
static uint64_t ap_boot=1,ap_run=10,ap_deadline=1000000,ap_freeze;
static unsigned ap_state=PM_AP_ARMED,ap_writers;
static uint32_t ap_generation=1;
bool pm_ack_probe_capture(uint64_t*b,uint64_t*r,uint64_t*d,unsigned*s,uint64_t*f,uint32_t*g){assert(lwip_depth==1);*b=ap_boot;*r=ap_run;*d=ap_deadline;*s=ap_state;*f=ap_freeze;*g=ap_generation;return ap_state==PM_AP_RECORDING;}
void pm_ack_probe_before_lock(uint8_t k,const char*j){(void)k;(void)j;ap_writers++;}
void pm_ack_probe_connection(void){ap_writers++;}
void pm_ack_probe_encoded(uint8_t k,const char*j){(void)k;(void)j;ap_writers++;}
void pm_ack_probe_locked(uint8_t k){(void)k;ap_writers++;}
void pm_ack_probe_net(pm_ack_probe_event e,uint16_t f,uint32_t a,uint32_t b,uint32_t c,uint32_t d){(void)e;(void)f;(void)a;(void)b;(void)c;(void)d;ap_writers++;}
bool pm_ack_probe_sample_due(void){return false;}
void pm_ack_probe_sample(uint16_t f,uint32_t a,uint32_t b,uint32_t c,uint32_t d){(void)f;(void)a;(void)b;(void)c;(void)d;ap_writers++;}
void pm_ack_probe_send_return(uint8_t k,bool ok){(void)k;(void)ok;ap_writers++;}
static struct netif first,second;
static struct pbuf head,tail;
static uint8_t frame[64];
static unsigned calls1,calls2;static int original_result;
static err_t original1(struct netif*n,struct pbuf*p){assert(lwip_depth==1&&n==&first&&p==&head);calls1++;
#if PM_CYW43_INNER_PROBE
 static uint8_t inner_buffer[128];inner_buffer[14]=0x20;assert(pbuf_copy_partial(p,inner_buffer+18,64,0)==64);
 pm_cyw_probe_copied(64,64);uint32_t token=pm_cyw_probe_open(2,70,inner_buffer,sizeof inner_buffer,0,1,0);pm_cyw_probe_bus(token,inner_buffer,sizeof inner_buffer,0,1,1);pm_cyw_probe_finish(token,original_result,false);
#endif
 return original_result;}
static err_t original2(struct netif*n,struct pbuf*p){assert(lwip_depth==1&&n==&second&&p==&head);calls2++;return original_result;}
static void packet_init(void){
 memset(frame,0,sizeof frame);frame[12]=8;frame[14]=0x45;frame[17]=50;frame[18]=0x12;frame[19]=0x34;frame[23]=6;
 frame[26]=192;frame[29]=1;frame[30]=192;frame[33]=2;frame[34]=0xc3;frame[35]=0x50;frame[36]=0x3f;frame[37]=0x19;frame[41]=1;frame[46]=0x50;frame[47]=0x18;
 head=(struct pbuf){.tot_len=64,.payload=frame,.next=&tail,.len=39,.ref=2,.flags=3};tail=(struct pbuf){.tot_len=25,.payload=frame+39,.len=25,.ref=1,.flags=7};
 socket_instance.local_ip.addr=inet_addr("192.0.0.1");socket_instance.remote_ip.addr=inet_addr("192.0.0.2");socket_instance.local_port=50000;socket_instance.remote_port=16153;socket_instance.nrtx=2;pcb=&socket_instance;
}
static void invoke(void){cyw43_arch_lwip_begin();assert(first.linkoutput(&first,&head)==original_result);cyw43_arch_lwip_end();}
static pthread_barrier_t before_reset,after_reset;
static void *writer(void *unused){(void)unused;for(unsigned i=0;i<1500;i++)invoke();pthread_barrier_wait(&before_reset);pthread_barrier_wait(&after_reset);for(unsigned i=0;i<1500;i++)invoke();return NULL;}
#ifndef PM_TX_ADAPTER_MAIN
#define PM_TX_ADAPTER_MAIN main
#endif
int PM_TX_ADAPTER_MAIN(void){
 packet_init();first.linkoutput=original1;second.linkoutput=original2;netif_default=&first;clock_us=100;
 cyw43_arch_lwip_begin();tx_attach();tx_attach();tx_sync();cyw43_arch_lwip_end();assert(first.linkoutput==tx_wrapper&&tx_original==original1);
 struct pbuf saved_head=head,saved_tail=tail;uint8_t saved[64];memcpy(saved,frame,64);
 ap_state=PM_AP_RECORDING;pm_tx_status s;pm_net_tx_status(&s);assert(s.active);
 for(int rc=0;rc>=-16;rc--){original_result=rc;invoke();}
 assert(calls1==17&&tx_probe.count==17&&!ap_writers);assert(!memcmp(&head,&saved_head,sizeof head)&&!memcmp(&tail,&saved_tail,sizeof tail)&&!memcmp(saved,frame,64));
 assert(tx_probe.rows[0].ipid==0x1234&&tx_probe.rows[0].nrtx==2&&tx_probe.rows[0].pbuf_ref==2);
 pm_tx_row row;assert(!pm_net_tx_read(1,10,0,&row));
 /* Snapshot/reset and callback contend under the same lock: no AP2 IRQ writers. */
 original_result=0;pthread_t worker;assert(!pthread_barrier_init(&before_reset,NULL,2));assert(!pthread_barrier_init(&after_reset,NULL,2));assert(!pthread_create(&worker,NULL,writer,NULL));
 for(unsigned i=0;i<1000;i++){
  pm_net_tx_status(&s);assert(s.boot==1&&s.run==ap_run&&s.count<=32);
 }
 pthread_barrier_wait(&before_reset);cyw43_arch_lwip_begin();ap_run=11;tx_sync();cyw43_arch_lwip_end();pthread_barrier_wait(&after_reset);
 assert(!pthread_join(worker,NULL));pm_net_tx_status(&s);assert(s.run==11&&s.count==32&&s.dropped>0&&!ap_writers);
#if PM_CYW43_INNER_PROBE
 pm_cyw_status inner;pm_net_cyw_status(&inner);assert(inner.run==11&&inner.count==PM_CYW_INNER_CAP&&inner.dropped>0&&!inner.incomplete);
#endif
 /* Late callbacks after main freeze cannot contaminate frozen rows/digest. */
 cyw43_arch_lwip_begin();ap_state=PM_AP_FROZEN;ap_freeze=clock_us;cyw43_arch_lwip_end();invoke();pm_net_tx_status(&s);assert(!s.active&&s.count==0);uint32_t digest=s.digest;invoke();pm_net_tx_status(&s);assert(s.digest==digest);
 /* Release retains TX, new run clears, generation change disables capture. */
 cyw43_arch_lwip_begin();ap_state=PM_AP_IDLE;ap_run=0;cyw43_arch_lwip_end();pm_net_tx_status(&s);assert(s.run==11);
 cyw43_arch_lwip_begin();ap_run=12;ap_state=PM_AP_RECORDING;ap_generation++;cyw43_arch_lwip_end();pm_net_tx_status(&s);invoke();assert(tx_probe.count==1);
 cyw43_arch_lwip_begin();ap_generation++;cyw43_arch_lwip_end();pm_net_tx_status(&s);assert(!s.active&&s.generation_changed);
 cyw43_arch_lwip_begin();tx_detach();assert(first.linkoutput==original1);tx_attach();tx_attach();assert(tx_original==original1);netif_default=&second;tx_attach();assert(first.linkoutput==original1&&second.linkoutput==tx_wrapper&&tx_original==original2);assert(second.linkoutput(&second,&head)==0);tx_detach();assert(second.linkoutput==original2);cyw43_arch_lwip_end();assert(calls2==1);
 puts("PASS production TX adapter: chained pbuf/ref/content/return preserved, original once, SDK-lock threaded snapshots/reset, full/drop, freeze/release/run/generation, attach/detach without recursion; no AP2 callback writes");
 return 0;
}
