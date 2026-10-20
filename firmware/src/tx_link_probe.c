#include "tx_link_probe.h"
#if PM_TX_LINK_PROBE
#include <string.h>
#include <limits.h>
static uint16_t be16(const uint8_t *p){return (uint16_t)((unsigned)p[0]<<8|p[1]);}
static uint32_t be32(const uint8_t *p){return (uint32_t)p[0]<<24|(uint32_t)p[1]<<16|(uint32_t)p[2]<<8|p[3];}
static bool header(pm_tx_probe *s,pm_tx_reader read,const void *packet,pm_tx_row *row){
    uint8_t type[2],ip[20],tcp[20];unsigned offset=14;
    if(!read(packet,12,type,2))return false;
    for(unsigned tags=0;(be16(type)==0x8100 || be16(type)==0x88a8) && tags<2;tags++){
        if(!read(packet,offset+2,type,2))return false;
        offset+=4;
    }
    if(be16(type)!=0x0800 || !read(packet,offset,ip,sizeof ip))return false;
    if(ip[0]>>4!=4 || ip[9]!=6 || memcmp(ip+12,s->tuple.local,4) || memcmp(ip+16,s->tuple.peer,4))return false;
    unsigned ihl=(ip[0]&15u)*4u;
    if(ihl<20 || ihl>60 || (be16(ip+6)&0x3fff) || !read(packet,offset+ihl,tcp,sizeof tcp)){s->malformed++;return false;}
    if(be16(tcp)!=s->tuple.local_port || be16(tcp+2)!=s->tuple.peer_port)return false;
    unsigned tcp_hlen=(tcp[12]>>4)*4u,total=be16(ip+2);
    if(tcp_hlen<20 || total<ihl+tcp_hlen || !read(packet,offset+total,NULL,0)){s->malformed++;return false;}
    unsigned payload=total-ihl-tcp_hlen;
    if(!payload)return false;
    row->seq=be32(tcp+4);row->end=row->seq+payload+!!(tcp[13]&2)+!!(tcp[13]&1);row->ack=be32(tcp+8);
    row->ipid=be16(ip+4);row->length=(uint16_t)payload;row->tcp_flags=tcp[13];return true;
}
int pm_tx_observe(pm_tx_probe *s,pm_tx_reader read,const void *packet,pm_tx_forwarder forward,void *context,pm_tx_clock clock,uint8_t nrtx,uint8_t refs){
    pm_tx_row row={0};bool keep=false;
    if(s->active){row.time_us=clock();keep=row.time_us<s->deadline_us && header(s,read,packet,&row);}
    /* Exactly one call with the identical packet. Never inspect it afterwards. */
    int result=forward(context,packet);
    if(keep){
        uint64_t elapsed=clock()-row.time_us;row.duration_us=elapsed>UINT32_MAX?UINT32_MAX:(uint32_t)elapsed;
        row.result=(int8_t)result;row.nrtx=nrtx;row.pbuf_ref=refs;
        if(s->count<PM_TX_LINK_CAP)s->rows[s->count++]=row;
        else if(s->dropped<UINT32_MAX)s->dropped++;
    }
    return result;
}
static uint32_t word(uint32_t h,uint64_t v,unsigned n){for(unsigned i=0;i<n;i++){h=(h^(uint8_t)v)*16777619u;v>>=8;}return h;}
uint32_t pm_tx_digest(const pm_tx_probe *s){
    uint32_t h=2166136261u;
    for(unsigned i=0;i<s->count;i++){
        const pm_tx_row *r=&s->rows[i];h=word(h,r->time_us,8);
        h=word(h,r->seq,4);h=word(h,r->end,4);h=word(h,r->ack,4);h=word(h,r->duration_us,4);
        h=word(h,r->ipid,2);h=word(h,r->length,2);h=word(h,(uint8_t)r->result,1);
        h=word(h,r->nrtx,1);h=word(h,r->tcp_flags,1);h=word(h,r->pbuf_ref,1);
    }
    return h;
}
#endif
