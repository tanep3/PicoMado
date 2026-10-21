/* Real AP2 + production pm_net_send/tx_sync + synchronous linkoutput + CY hooks. */
#pragma GCC diagnostic ignored "-Wmissing-field-initializers"
#define main existing_net_cases
#include "net_socks_test.c"
#undef main
struct netif *netif_default;
static struct netif hb_netif;
static uint8_t hb_packet[256],hb_inner[320];
static struct pbuf hb_pbuf;
static unsigned hb_calls;
static uint16_t hb_ipid;
const char pm_cyw43_inner_translation_unit[]="host-fixture";
static void hb_word(unsigned at,unsigned value){hb_packet[at]=value>>8;hb_packet[at+1]=value;}
static void hb_long(unsigned at,uint32_t value){for(unsigned i=0;i<4;i++)hb_packet[at+i]=(uint8_t)(value>>(24-8*i));}
static err_t hb_original(struct netif*n,struct pbuf*p){
 assert(n==&hb_netif&&p==&hb_pbuf&&lwip_depth==1);hb_calls++;
 pm_cyw_status state;pm_cyw_probe_status(&state);assert(state.active);
 memset(hb_inner,0,sizeof hb_inner);hb_inner[14]=0x20;
 memcpy(hb_inner+18,hb_packet,p->tot_len);pm_cyw_probe_copied(p->tot_len,p->tot_len);
 uint32_t ticket=pm_cyw_probe_open(2,p->tot_len+6,hb_inner,sizeof hb_inner,0,1,0);
 assert(ticket);pm_cyw_probe_bus(ticket,hb_inner,sizeof hb_inner,0,1,1);pm_cyw_probe_finish(ticket,0,false);
 return 0;
}
static void hb_output(void){
 assert(tx_probe.active);memset(hb_packet,0,sizeof hb_packet);hb_packet[12]=8;hb_packet[14]=0x45;
 hb_word(16,40+sent_length);hb_word(18,++hb_ipid);hb_packet[23]=6;
 memcpy(hb_packet+26,&socket_instance.local_ip.addr,4);memcpy(hb_packet+30,&socket_instance.remote_ip.addr,4);
 hb_word(34,socket_instance.local_port);hb_word(36,socket_instance.remote_port);
 hb_long(38,socket_instance.snd_lbb-sent_length);hb_long(42,450);hb_packet[46]=0x50;hb_packet[47]=0x18;
 memcpy(hb_packet+54,sent,sent_length);
 hb_pbuf=(struct pbuf){.payload=hb_packet,.len=54+sent_length,.tot_len=54+sent_length,.ref=1};
 assert(hb_netif.linkoutput(&hb_netif,&hb_pbuf)==0);
}
static void hb_drain(void){for(unsigned i=0;i<20;i++){clock_us+=25000;pm_ack_probe_poll(true);pm_trace_flush();}}
static void hb_prepare(unsigned run){
 pm_ack_probe_init_boot(1);pm_ack_probe_connection();pm_ack_probe_live(9,true);pm_ack_probe_usb_state(true);
 char command[80];snprintf(command,sizeof command,"AP2 ARM_HB 1 %x",run);pm_ack_probe_command(command);hb_drain();
 pm_ack_probe_usb_state(false);socket_instance=(struct tcp_pcb){0};
 socket_instance.local_ip.addr=inet_addr("192.0.0.1");socket_instance.remote_ip.addr=inet_addr("192.0.0.2");
 socket_instance.local_port=50000;socket_instance.remote_port=16153;
 socket_instance.snd_lbb=socket_instance.snd_nxt=socket_instance.lastack=100;socket_instance.flags=TF_NODELAY;
 pcb=&socket_instance;stage=PM_NET_READY;ws_closed=false;send_room=4096;write_result=output_result=0;
 hb_netif.linkoutput=hb_original;netif_default=&hb_netif;tx_netif=NULL;tx_original=NULL;
 cyw43_arch_lwip_begin();tx_attach();tx_sync();cyw43_arch_lwip_end();
 test_output_hook=hb_output;hb_calls=0;
}
int main(void){
 clock_us=100;hb_prepare(10);assert(!tx_probe.active);
 assert(pm_net_send(5,"{}"));assert(hb_calls==1&&tx_probe.count==1&&tx_probe.active);
 pm_ack_probe_record row;assert(pm_ack_probe_read(0,&row)&&row.event==25&&row.message==1&&row.session==9);
 unsigned copy_at=999,output_at=999;
 for(unsigned i=0;pm_ack_probe_read(i,&row);i++){if(row.event==PM_AP_TX_POSITION){copy_at=i;assert(row.a==100&&row.b==114&&row.d==100);}if(row.event==PM_AP_OUTPUT_END)output_at=i;}
 assert(copy_at<output_at);pm_cyw_status cy;pm_cyw_probe_status(&cy);assert(cy.count==1&&!cy.dropped);
 // A later retransmission uses the same wrapper exactly once, with its own row.
 cyw43_arch_lwip_begin();hb_output();cyw43_arch_lwip_end();assert(hb_calls==2&&tx_probe.count==2);
 pm_cyw_probe_status(&cy);assert(cy.count==2&&!cy.incomplete);
 pm_ack_probe_summary summary=pm_ack_probe_read_summary();clock_us=summary.trigger_us+PM_ACK_PROBE_WINDOW_US;
 pm_ack_probe_poll(false);assert(pm_ack_probe_read_summary().state==PM_AP_FROZEN);
 pm_tx_status tx;pm_net_tx_status(&tx);assert(!tx.active&&tx.count==2);
 puts("PASS real heartbeat COPY before synchronous original-once TX/CY and later retransmission; fixed deadline");
 hb_prepare(11);write_result=ERR_MEM;assert(!pm_net_send(5,"{}"));assert(stage==PM_NET_FAILED&&!hb_calls&&!tx_probe.count);
 assert(pm_ack_probe_read(0,&row)&&row.event==25);bool copy_seen=false;
 for(unsigned i=0;pm_ack_probe_read(i,&row);i++)if(row.event==PM_AP_TX_POSITION)copy_seen=true;
 assert(!copy_seen);puts("PASS real failed COPY retained without invented TX/CY or successful bool");
 return 0;
}
