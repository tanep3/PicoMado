#include <assert.h>
#include <stdlib.h>
#include <stdio.h>
#include <string.h>
#include <stdbool.h>
#include <stdint.h>
#include "lwip/priv/tcp_priv.h"
#include "lwip/ip4.h"
#include "lwip/netif.h"
static uint64_t clock_us=1000;
uint64_t time_us_64(void){return clock_us++;}
struct netif *netif_default;
struct tcp_pcb *tcp_input_pcb;
u32_t tcp_ticks;
static unsigned outputs;
static err_t output_result;
static bool fail_alloc;
static uint16_t get16(const uint8_t *p){return (uint16_t)((unsigned)p[0]<<8|p[1]);}
static void put16(uint8_t *p,unsigned n){p[0]=(uint8_t)(n>>8);p[1]=(uint8_t)n;}
static void put32(uint8_t *p,uint32_t n){p[0]=(uint8_t)(n>>24);p[1]=(uint8_t)(n>>16);p[2]=(uint8_t)(n>>8);p[3]=(uint8_t)n;}
u16_t pbuf_copy_partial(const struct pbuf *p,void *dst,u16_t n,u16_t offset){
    unsigned copied=0;
    for(;p && copied<n;p=p->next){
        if(offset>=p->len){offset=(u16_t)(offset-p->len);continue;}
        unsigned available=p->len-offset,part=n-copied;if(part>available)part=available;
        memcpy((uint8_t *)dst+copied,(uint8_t *)p->payload+offset,part);copied+=part;offset=0;
    }
    return (u16_t)copied;
}
struct pbuf *pbuf_alloc(pbuf_layer layer,u16_t length,pbuf_type type){
    (void)layer;(void)type;if(fail_alloc)return NULL;struct pbuf *p=calloc(1,sizeof *p+length+128);assert(p);p->payload=(uint8_t *)(p+1)+64;p->len=p->tot_len=length;p->ref=1;return p;
}
u8_t pbuf_free(struct pbuf *p){free(p);return 1;}
void pbuf_ref(struct pbuf *p){p->ref++;}
u16_t ip_chksum_pseudo(struct pbuf *p,u8_t proto,u16_t len,const ip_addr_t *src,const ip_addr_t *dst){(void)p;(void)proto;(void)len;(void)src;(void)dst;return 0;}
u16_t ip_chksum_pseudo_partial(struct pbuf *p,u8_t proto,u16_t len,u16_t covered,const ip_addr_t *src,const ip_addr_t *dst){(void)covered;return ip_chksum_pseudo(p,proto,len,src,dst);}
struct netif *ip4_route(const ip4_addr_t *dest){(void)dest;return netif_default;}
struct netif *netif_get_by_index(u8_t index){(void)index;return netif_default;}
u16_t tcp_eff_send_mss_netif(u16_t mss,struct netif *n,const ip_addr_t *dest){(void)n;(void)dest;return mss;}
void tcp_seg_free(struct tcp_seg *seg){(void)seg;}
static err_t actual_link(struct netif *n,struct pbuf *p){(void)n;assert(p->tot_len>=54);outputs++;return output_result;}
err_t ip4_output_if(struct pbuf *p,const ip4_addr_t *src,const ip4_addr_t *dst,u8_t ttl,u8_t tos,u8_t proto,struct netif *n){
    (void)ttl;(void)tos;assert(proto==6);uint8_t bytes[128]={0};assert(p->tot_len+34u<=sizeof bytes);
    put16(bytes+12,0x0800);bytes[14]=0x45;bytes[23]=6;
    put32(bytes+26,lwip_ntohl(ip4_addr_get_u32(src)));put32(bytes+30,lwip_ntohl(ip4_addr_get_u32(dst)));
    assert(pbuf_copy_partial(p,bytes+34,p->tot_len,0)==p->tot_len);
    struct pbuf eth={.payload=bytes,.len=(u16_t)(p->tot_len+34),.tot_len=(u16_t)(p->tot_len+34)};
    assert(get16(bytes+34)!=0);return n->linkoutput(n,&eth);
}
static void setup(struct tcp_pcb *p,uint16_t port){
    memset(p,0,sizeof *p);IP4_ADDR(ip_2_ip4(&p->local_ip),192,168,0,173);IP4_ADDR(ip_2_ip4(&p->remote_ip),192,168,0,120);
    p->local_port=port;p->remote_port=16153;p->state=ESTABLISHED;p->rcv_nxt=183866347;p->rcv_wnd=p->rcv_ann_wnd=23360;p->snd_nxt=77;p->lastack=75;p->snd_wnd=p->cwnd=23360;p->mss=1460;p->rtime=-1;
}
int main(int argc,char **argv){
 assert(argc==3);unsigned application_flags=0;FILE *input=fopen(argv[1],"r");assert(input);assert(fscanf(input,"%u",&application_flags)==1);fclose(input);
 bool expect_output=atoi(argv[2])!=0;assert((application_flags&~TF_NODELAY)==0);
 struct netif n={.linkoutput=actual_link};netif_default=&n;struct tcp_pcb a;setup(&a,61209);output_result=ERR_OK;
 struct pbuf *packet=pbuf_alloc(PBUF_IP,24,PBUF_RAM);struct tcp_hdr *h=packet->payload;memset(h,0,24);h->src=lwip_htons(a.local_port);h->dest=lwip_htons(a.remote_port);h->seqno=lwip_htonl(77);TCPH_HDRLEN_FLAGS_SET(h,5,TCP_PSH);
 struct tcp_hdr old_h={.seqno=lwip_htonl(75)};struct tcp_seg old={.len=2,.tcphdr=&old_h},seg={.p=packet,.tcphdr=h,.len=4};a.unsent=&seg;a.unacked=&old;a.snd_buf=TCP_SND_BUF-6;a.snd_queuelen=2;a.flags=(tcpflags_t)(application_flags|TF_ACK_NOW);
 assert(tcp_output(&a)==ERR_OK);
 if(expect_output){
  assert(outputs==1 && a.unsent==NULL && !(a.flags&(TF_ACK_NOW|TF_ACK_DELAY)));assert(lwip_ntohl(h->ackno)==a.rcv_nxt);assert(TCPH_FLAGS(h)&TCP_ACK);
 }else{
  assert(outputs==0 && a.unsent==&seg && (a.flags&TF_ACK_NOW));
  for(unsigned i=0;i<100;i++)assert(tcp_output(&a)==ERR_OK);
  assert(outputs==0 && (a.flags&TF_ACK_NOW));
 }
 assert((a.flags&TF_NODELAY)==application_flags);pbuf_free(packet);
 unsigned first_outputs=outputs;
 // Control: without pending application data, the original default Nagle PCB
 // can already send a pure ACK. The tested hold condition requires both queues.
 setup(&a,61210);a.flags=TF_ACK_NOW;assert(tcp_output(&a)==ERR_OK);assert(outputs==first_outputs+1 && !(a.flags&TF_ACK_NOW));
 printf("PASS actual SDK tcp_output: application_flags=%u small-unacked+unsent emitted=%u, ACK_NOW_pending=%u; empty-unsent pure ACK succeeds\n",application_flags,first_outputs,!expect_output);
 return 0;
}
