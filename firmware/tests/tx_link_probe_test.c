#include <assert.h>
#include <stdio.h>
#include <string.h>
#include "tx_link_probe.h"
typedef struct {uint8_t a[45],b[160];unsigned length,reads;void *last;unsigned calls;int ret;} packet;
static uint64_t clock_value;
static uint64_t clock_fn(void){clock_value+=7;return clock_value;}
static bool read_packet(const void *ptr,unsigned offset,void *out,unsigned n){
 packet *p=(packet *)ptr;p->reads++;if(offset>p->length || n>p->length-offset)return false;
 for(unsigned i=0;i<n;i++){unsigned pos=offset+i;((uint8_t *)out)[i]=pos<45?p->a[pos]:p->b[pos-45];}return true;
}
static int forward(void *ctx,const void *ptr){packet *p=ctx;assert(p==ptr);p->calls++;p->last=(void *)ptr;return p->ret;}
static void put(packet *p,unsigned pos,uint8_t v){if(pos<45)p->a[pos]=v;else p->b[pos-45]=v;}
static void word(packet *p,unsigned pos,unsigned v){put(p,pos,v>>8);put(p,pos+1,v);}
static void dword(packet *p,unsigned pos,uint32_t v){put(p,pos,v>>24);put(p,pos+1,v>>16);put(p,pos+2,v>>8);put(p,pos+3,v);}
static packet make(unsigned tags,unsigned ihl,unsigned thl){
 packet p={.ret=-12};unsigned at=12;
 for(unsigned i=0;i<tags;i++){word(&p,at,0x8100);at+=4;}
 word(&p,at,0x0800);unsigned ip=14+tags*4,tcp=ip+ihl;p.length=ip+ihl+thl+10;
 put(&p,ip,0x40+ihl/4);put(&p,ip+9,6);word(&p,ip+2,ihl+thl+10);word(&p,ip+4,1234);
 put(&p,ip+12,192);put(&p,ip+15,1);put(&p,ip+16,192);put(&p,ip+19,2);
 word(&p,tcp,50000);word(&p,tcp+2,16153);dword(&p,tcp+4,0xfffffff8);dword(&p,tcp+8,777);put(&p,tcp+12,thl/4*16);put(&p,tcp+13,24);return p;
}
static pm_tx_probe probe(void){return (pm_tx_probe){.active=true,.deadline_us=100000,.tuple={.local={192,0,0,1},.peer={192,0,0,2},.local_port=50000,.peer_port=16153}};}
int main(void){
 for(unsigned tags=0;tags<3;tags++)for(unsigned ihl=20;ihl<=60;ihl+=40)for(unsigned thl=20;thl<=32;thl+=12){
  pm_tx_probe s=probe();packet p=make(tags,ihl,thl);uint8_t olda[45],oldb[160];memcpy(olda,p.a,sizeof olda);memcpy(oldb,p.b,sizeof oldb);
  int rc=pm_tx_observe(&s,read_packet,&p,forward,&p,clock_fn,2,1);assert(rc==-12&&p.calls==1&&p.last==&p&&s.count==1);
  assert(!memcmp(olda,p.a,sizeof olda)&&!memcmp(oldb,p.b,sizeof oldb));assert(s.rows[0].seq==0xfffffff8&&s.rows[0].end==2&&s.rows[0].ack==777&&s.rows[0].ipid==1234&&s.rows[0].length==10&&s.rows[0].result==-12&&s.rows[0].nrtx==2&&s.rows[0].pbuf_ref==1);
  assert(p.reads<=6); // header peeks and size query only; no payload scan.
 }
 pm_tx_probe s=probe();packet p=make(0,20,20);word(&p,12,0x0806);assert(pm_tx_observe(&s,read_packet,&p,forward,&p,clock_fn,0,1)==-12&&p.calls==1&&!s.count);
 s=probe();p=make(0,20,20);p.length=10;pm_tx_observe(&s,read_packet,&p,forward,&p,clock_fn,0,1);assert(p.calls==1&&!s.count);
 s=probe();p=make(0,20,20);word(&p,14+6,1);pm_tx_observe(&s,read_packet,&p,forward,&p,clock_fn,0,1);assert(p.calls==1&&!s.count&&s.malformed==1);
 s=probe();p=make(0,20,20);p.length--;pm_tx_observe(&s,read_packet,&p,forward,&p,clock_fn,0,1);assert(p.calls==1&&!s.count&&s.malformed==1);
 s=probe();p=make(0,20,20);word(&p,34,123);pm_tx_observe(&s,read_packet,&p,forward,&p,clock_fn,0,1);assert(p.calls==1&&!s.count);
 s=probe();p=make(0,20,20);s.active=false;pm_tx_observe(&s,read_packet,&p,forward,&p,clock_fn,0,1);assert(p.calls==1&&!p.reads&&!s.count);
 s=probe();p=make(0,20,20);s.deadline_us=clock_value;pm_tx_observe(&s,read_packet,&p,forward,&p,clock_fn,0,1);assert(p.calls==1&&!p.reads&&!s.count);
 s=probe();p=make(0,20,20);p.ret=0;
 for(unsigned i=0;i<PM_TX_LINK_CAP+3;i++)assert(pm_tx_observe(&s,read_packet,&p,forward,&p,clock_fn,0,1)==0);
 assert(p.calls==PM_TX_LINK_CAP+3&&s.count==PM_TX_LINK_CAP&&s.dropped==3);uint32_t h=pm_tx_digest(&s);assert(h==pm_tx_digest(&s));s.rows[0].result=-1;assert(h!=pm_tx_digest(&s));
 puts("PASS bounded link observer: same pointer/one call/error preserved; chained/header/VLAN/IP/TCP options/fragment/short/wrap; no payload copy; no overwrite and explicit drops");
}
