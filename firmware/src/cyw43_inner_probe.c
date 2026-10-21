#include "cyw43_inner_probe.h"
#if PM_CYW43_INNER_PROBE
#include "pico/stdlib.h"
#include <string.h>
#include <limits.h>
static struct {
 uint64_t boot,run,deadline;
 pm_tx_tuple tuple;
 uint32_t generation,serial,depth,stack[PM_CYW_INNER_DEPTH];
 uint32_t count,dropped,incomplete,saturated,depth_lost,rejected,skipped;
 uint32_t reads,requested,errors,crc_calls,crc_bytes,crc_us,crc_max_us;
 uint32_t copy_expected,copy_actual;
 bool active,changed,copy_valid;
 pm_cyw_row rows[PM_CYW_INNER_CAP];
} probe;
_Static_assert(PM_CYW_INNER_CAP<=15,"ticket slot is four bits, zero reserved");
_Static_assert(sizeof(pm_cyw_key)==20,"explicit key layout");
_Static_assert(sizeof(pm_cyw_row)<=144,"fixed row budget");
_Static_assert(sizeof probe<=2544,"inner probe BSS budget");
static void inc(uint32_t *v){if(*v==UINT32_MAX)probe.saturated=1;else ++*v;}
static void add(uint32_t *v,uint32_t n){if(n>UINT32_MAX-*v){*v=UINT32_MAX;probe.saturated=1;}else *v+=n;}
static void inc16(uint16_t *v,pm_cyw_row *r){if(*v==UINT16_MAX){r->flags|=PM_CY_SAT;probe.saturated=1;}else ++*v;}
static uint16_t be16(const uint8_t*p){return(uint16_t)((unsigned)p[0]<<8|p[1]);}
static uint32_t be32(const uint8_t*p){return(uint32_t)p[0]<<24|(uint32_t)p[1]<<16|(uint32_t)p[2]<<8|p[3];}
uint32_t pm_cyw_crc32(const uint8_t *p,size_t n){
 static const uint32_t t[16]={0,0x1db71064,0x3b6e20c8,0x26d930ac,0x76dc4190,0x6b6b51f4,0x4db26158,0x5005713c,0xedb88320,0xf00f9344,0xd6d6a3e8,0xcb61b38c,0x9b64c2b0,0x86d3d2d4,0xa00ae278,0xbdbdf21c};
 uint32_t c=UINT32_MAX;for(size_t i=0;i<n;i++){c^=p[i];c=(c>>4)^t[c&15];c=(c>>4)^t[c&15];}return~c;
}
static uint32_t crc(const uint8_t*p,unsigned n){
 uint64_t start=time_us_64();uint32_t value=pm_cyw_crc32(p,n);uint64_t took=time_us_64()-start;
 inc(&probe.crc_calls);add(&probe.crc_bytes,n);uint32_t elapsed=took>UINT32_MAX?UINT32_MAX:(uint32_t)took;add(&probe.crc_us,elapsed);if(elapsed>probe.crc_max_us)probe.crc_max_us=elapsed;return value;
}
/* Read only initialized Ethernet bytes. Parse options by length, no option/payload scan. */
static bool key(const uint8_t*p,size_t n,pm_cyw_key*k,bool *matches){
 *k=(pm_cyw_key){0};*matches=false;if(n<14)return false;unsigned at=14;k->type=be16(p+12);
 for(unsigned tags=0;(k->type==0x8100||k->type==0x88a8)&&tags<2;tags++){if(n<at+4)return false;k->type=be16(p+at+2);at+=4;}
 if(k->type!=0x0800||n-at<20)return false;
 const uint8_t*ip=p+at;unsigned ihl=(ip[0]&15u)*4,total=be16(ip+2);
 if(ip[0]>>4!=4||ip[9]!=6||ihl<20||ihl>60||(be16(ip+6)&0x3fff)||total<ihl+20||total>n-at)return false;
 const uint8_t*tcp=ip+ihl;unsigned thl=(tcp[12]>>4)*4;
 if(thl<20||thl>60||total<ihl+thl)return false;
 k->seq=be32(tcp+4);k->ack=be32(tcp+8);k->ipid=be16(ip+4);k->length=(uint16_t)(total-ihl-thl);k->flags=tcp[13];k->end=k->seq+k->length+!!(k->flags&2)+!!(k->flags&1);k->valid=1;
 *matches=!memcmp(ip+12,probe.tuple.local,4)&&!memcmp(ip+16,probe.tuple.peer,4)&&be16(tcp)==probe.tuple.local_port&&be16(tcp+2)==probe.tuple.peer_port;return true;
}
static bool different(const pm_cyw_key*a,const pm_cyw_key*b){return a->seq!=b->seq||a->end!=b->end||a->ack!=b->ack||a->ipid!=b->ipid||a->length!=b->length||a->type!=b->type||a->flags!=b->flags||a->valid!=b->valid;}
static pm_cyw_row *row(uint32_t ticket){unsigned slot=ticket&15u;if(!slot||slot>probe.count)return NULL;pm_cyw_row*r=&probe.rows[slot-1];return r->ticket==ticket&&!(r->flags&PM_CY_DONE)?r:NULL;}
static bool view(pm_cyw_row*r,const uint8_t*p,size_t capacity){if(!p||capacity<PM_CYW_INNER_OFFSET||r->domain>capacity-PM_CYW_INNER_OFFSET){r->flags|=PM_CY_INVALID;inc(&probe.incomplete);return false;}return true;}
void pm_cyw_probe_sync(uint64_t boot,uint64_t run,uint32_t generation,bool active,bool connection_changed,uint64_t deadline,uint64_t frozen,const pm_tx_tuple *tuple){
 if(run&&run!=probe.run){
  if(probe.depth){probe.active=false;probe.changed=true;inc(&probe.incomplete);return;}
  uint32_t serial=probe.serial;memset(&probe,0,sizeof probe);probe.serial=serial;probe.boot=boot;probe.run=run;probe.generation=generation;probe.tuple=*tuple;
 }
 if(run==probe.run&&run&&(boot!=probe.boot||generation!=probe.generation||connection_changed))probe.changed=true;
 if(frozen&&run==probe.run){
  if(probe.depth){probe.active=false;inc(&probe.incomplete);return;}
  while(probe.count&&probe.rows[probe.count-1].enter_us>=frozen)probe.count--;
 }
 probe.deadline=deadline;probe.active=active&&run==probe.run&&boot==probe.boot&&!probe.changed&&time_us_64()<deadline;
}
uint32_t pm_cyw_probe_open(unsigned kind,size_t length,const uint8_t*p,size_t capacity,uint8_t flow,uint8_t credit,uint8_t sequence){
 if(!probe.active||time_us_64()>=probe.deadline)return 0;
 if(probe.depth==PM_CYW_INNER_DEPTH){inc(&probe.depth_lost);return 0;}
 if(probe.serial>=0x0fffffff){probe.saturated=1;return 0;}
 for(unsigned i=0;i<probe.depth;i++){pm_cyw_row*r=row(probe.stack[i]);if(r){r->flags|=PM_CY_NESTED;if(r->nested==UINT8_MAX){probe.saturated=1;r->flags|=PM_CY_SAT;}else r->nested++;}}
 uint32_t ticket=(++probe.serial)<<4;unsigned index=probe.depth++;probe.stack[index]=ticket;
 if(kind!=2)return ticket;
 bool copied=probe.copy_valid&&probe.copy_expected==probe.copy_actual&&length>=6&&probe.copy_expected==length-6;probe.copy_valid=false;
 if(!copied){inc(&probe.rejected);inc(&probe.incomplete);return ticket;}
 if(!p||capacity<18||length<20||length>capacity-12||length-2>UINT16_MAX){inc(&probe.rejected);return ticket;}
 pm_cyw_key initial;bool matches;
 if(!key(p+18,length-6,&initial,&matches)||!matches||!initial.length||p[16]!=0){inc(&probe.skipped);return ticket;}
 if(probe.count==PM_CYW_INNER_CAP){inc(&probe.dropped);return ticket;}
 unsigned slot=probe.count++;ticket|=slot+1;probe.stack[index]=ticket;pm_cyw_row*r=&probe.rows[slot];memset(r,0,sizeof *r);
 r->ticket=ticket;r->enter_us=time_us_64();r->original=initial;r->domain=(uint16_t)(length-2);r->covered=r->domain>PM_CYW_INNER_CRC_CAP?PM_CYW_INNER_CRC_CAP:r->domain;r->flags=r->covered==r->domain?PM_CY_FULL:PM_CY_PREFIX;
 r->bdc_before=be32(p+14);r->flow_before=flow;r->credit_before=credit;r->sequence_before=sequence;r->depth=(uint8_t)probe.depth;r->io_calls=probe.reads;r->io_requested=probe.requested;r->io_errors=probe.errors;r->crc_before=crc(p+14,r->covered);return ticket;
}
void pm_cyw_probe_copied(size_t expected,size_t actual){
 if(!probe.active&&!probe.depth)return;
 if(expected>UINT32_MAX||actual>UINT32_MAX){probe.copy_valid=false;inc(&probe.rejected);probe.saturated=1;return;}
 probe.copy_expected=(uint32_t)expected;probe.copy_actual=(uint32_t)actual;probe.copy_valid=true;
}
void pm_cyw_probe_wait(uint32_t ticket){pm_cyw_row*r=row(ticket);if(r){r->flags|=PM_CY_WAIT;r->wait_us=(uint32_t)time_us_64();}}
void pm_cyw_probe_read(size_t requested,int result){
 if(!probe.depth)return;
 inc(&probe.reads);if(requested>UINT32_MAX){probe.saturated=1;probe.requested=UINT32_MAX;}else add(&probe.requested,(uint32_t)requested);if(result)inc(&probe.errors);
}
void pm_cyw_probe_poll(uint32_t ticket,int result,size_t valid_length,const uint8_t*p,size_t capacity){
 pm_cyw_row*r=row(ticket);if(!r)return;inc(&r->polls);
 if(result==2)inc16(&r->rx_data,r);else if(result==0)inc16(&r->rx_control,r);else if(result==1)inc16(&r->rx_event,r);else if(result!=-1)inc16(&r->rx_bad,r);
 /* SPI may have modified the buffer even if parsing/read failed. No res_buf use. */
 if(!(r->flags&PM_CY_RX_CRC)&&probe.reads!=r->io_calls&&view(r,p,capacity)){
  r->crc_rx=crc(p+14,r->covered);r->flags|=PM_CY_RX_CRC;if(r->crc_rx!=r->crc_before)r->flags|=PM_CY_RX_CHANGED;
  if(result<INT16_MIN||result>INT16_MAX){r->rx_result=result<0?INT16_MIN:INT16_MAX;r->flags|=PM_CY_SAT;probe.saturated=1;}else r->rx_result=(int16_t)result;
  if(result>=0){
   bool interface1=result==2&&(valid_length&UINT32_C(0x80000000));
   if(result==2)valid_length&=UINT32_C(0x7fffffff);
   if(valid_length>0x7fff){r->rx_length=0x7fff;r->flags|=PM_CY_SAT;probe.saturated=1;}else r->rx_length=(uint16_t)valid_length;
   if(interface1)r->rx_length|=0x8000;
  }
 }
}
void pm_cyw_probe_bus(uint32_t ticket,const uint8_t*p,size_t capacity,uint8_t flow,uint8_t credit,uint8_t sequence){
 pm_cyw_row*r=row(ticket);if(!r)return;
 if(r->flags&PM_CY_WAIT)r->wait_us=(uint32_t)time_us_64()-r->wait_us;
 if(view(r,p,capacity)){
  r->crc_bus=crc(p+14,r->covered);bool matches;key(p+18,r->domain-4,&r->bus,&matches);r->bdc_bus=be32(p+14);
  if(r->crc_bus!=r->crc_before)r->flags|=PM_CY_CHANGED;
  if(different(&r->original,&r->bus))r->flags|=PM_CY_KEY_CHANGED;
  if(!r->bus.valid)r->flags|=PM_CY_BAD_BUS_KEY;
 }
 r->io_calls=probe.reads-r->io_calls;r->io_requested=probe.requested-r->io_requested;r->io_errors=probe.errors-r->io_errors;
 if(r->io_errors)r->flags|=PM_CY_IO_ERROR;
 if(probe.saturated)r->flags|=PM_CY_SAT;
 r->flow_bus=flow;r->credit_bus=credit;r->sequence_bus=sequence;r->flags|=PM_CY_BUS;r->bus_us=time_us_64();
}
void pm_cyw_probe_finish(uint32_t ticket,int result,bool timeout){
 if(!ticket)return;
 if(!probe.depth||probe.stack[probe.depth-1]!=ticket){if(probe.depth){inc(&probe.incomplete);probe.changed=true;probe.active=false;}return;}
 pm_cyw_row*r=row(ticket);
 if(r){
  if(!(r->flags&PM_CY_BUS)){r->io_calls=probe.reads-r->io_calls;r->io_requested=probe.requested-r->io_requested;r->io_errors=probe.errors-r->io_errors;if(r->flags&PM_CY_WAIT)r->wait_us=(uint32_t)time_us_64()-r->wait_us;}
  if(r->io_errors)r->flags|=PM_CY_IO_ERROR;
  r->result=result;r->end_us=time_us_64();if(timeout)r->flags|=PM_CY_TIMEOUT;if(probe.saturated)r->flags|=PM_CY_SAT;r->flags|=PM_CY_DONE;
 }
 probe.depth--;
}
void pm_cyw_probe_reject(void){if(probe.depth||(probe.active&&time_us_64()<probe.deadline))inc(&probe.rejected);}
static uint32_t word(uint32_t h,uint64_t v,unsigned n){for(unsigned i=0;i<n;i++){h=(h^(uint8_t)v)*16777619u;v>>=8;}return h;}
static uint32_t key_digest(uint32_t h,const pm_cyw_key*k){h=word(h,k->seq,4);h=word(h,k->end,4);h=word(h,k->ack,4);h=word(h,k->ipid,2);h=word(h,k->length,2);h=word(h,k->type,2);h=word(h,k->flags,1);return word(h,k->valid,1);}
static uint32_t digest(void){
 uint32_t h=2166136261u;for(unsigned i=0;i<probe.count;i++){pm_cyw_row*r=&probe.rows[i];h=word(h,r->enter_us,8);h=word(h,r->bus_us,8);h=word(h,r->end_us,8);h=key_digest(h,&r->original);h=key_digest(h,&r->bus);
 #define W(k,n) h=word(h,r->k,n)
 W(crc_before,4);W(crc_rx,4);W(crc_bus,4);W(bdc_before,4);W(bdc_bus,4);W(wait_us,4);W(polls,4);W(io_calls,4);W(io_requested,4);W(io_errors,4);W(ticket,4);W(result,4);W(flags,2);W(covered,2);W(domain,2);W(rx_data,2);W(rx_control,2);W(rx_event,2);W(rx_bad,2);W(rx_result,2);W(rx_length,2);W(flow_before,1);W(flow_bus,1);W(credit_before,1);W(credit_bus,1);W(sequence_before,1);W(sequence_bus,1);W(depth,1);W(nested,1);
 #undef W
 }return h;
}
void pm_cyw_probe_status(pm_cyw_status*s){
 uint32_t partial=0;for(unsigned i=0;i<probe.count;i++)if(!(probe.rows[i].flags&PM_CY_DONE))partial++;
 *s=(pm_cyw_status){.boot=probe.boot,.run=probe.run,.generation=probe.generation,.count=probe.count,.dropped=probe.dropped,.incomplete=partial>UINT32_MAX-probe.incomplete?UINT32_MAX:probe.incomplete+partial,.saturated=probe.saturated,.depth_lost=probe.depth_lost,.rejected=probe.rejected,.skipped=probe.skipped,.digest=digest(),.crc_calls=probe.crc_calls,.crc_bytes=probe.crc_bytes,.crc_us=probe.crc_us,.crc_max_us=probe.crc_max_us,.active=probe.active,.changed=probe.changed};
}
bool pm_cyw_probe_row(uint64_t boot,uint64_t run,unsigned i,pm_cyw_row*out){if(probe.active||boot!=probe.boot||run!=probe.run||i>=probe.count||!(probe.rows[i].flags&PM_CY_DONE))return false;*out=probe.rows[i];return true;}
#endif
