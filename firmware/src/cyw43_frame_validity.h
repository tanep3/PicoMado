#pragma once
#include <stddef.h>
#include <stdint.h>
#include <string.h>

/* Pure check of the bytes already assembled for one SDPCM DATA write.
   CHECKED and OK are distinct: malformed/truncated input is never a pass. */
enum {
 PM_FV_IP_CHECKED=1u, PM_FV_IP_OK=2u,
 PM_FV_TCP_CHECKED=4u, PM_FV_TCP_OK=8u,
 PM_FV_SDPCM_CHECKED=16u, PM_FV_SDPCM_OK=32u,
 PM_FV_MAC_PRESENT=64u
};
typedef struct { uint16_t checks; uint8_t dest_mac[6]; } pm_frame_validity;

static inline uint16_t pm_fv_be16(const uint8_t *p){return (uint16_t)((unsigned)p[0]<<8|p[1]);}
static inline uint16_t pm_fv_le16(const uint8_t *p){return (uint16_t)((unsigned)p[1]<<8|p[0]);}
static inline uint32_t pm_fv_sum(uint32_t sum,const uint8_t *p,size_t n){
 while(n>=2){sum+=pm_fv_be16(p);p+=2;n-=2;}
 if(n)sum+=(uint32_t)p[0]<<8;
 return sum;
}
static inline uint16_t pm_fv_fold(uint32_t sum){
 while(sum>>16)sum=(sum&0xffffu)+(sum>>16);
 return (uint16_t)sum;
}
/* Source: SDK cyw43_ll.c SDPCM12 + padding2 + BDC4 + Ethernet,
   cyw43_spi.c 4-byte padded WLAN transfer; lwIP IPv4/TCP checksums. */
static inline pm_frame_validity pm_cyw_validate_frame(const uint8_t *p,size_t capacity,
                                                      size_t ethernet_len,uint8_t sequence){
 pm_frame_validity out={0};
 if(!p || ethernet_len<14 || ethernet_len>capacity || capacity<18 || ethernet_len>capacity-18)return out;
 const uint8_t *eth=p+18;
 memcpy(out.dest_mac,eth,6);out.checks|=PM_FV_MAC_PRESENT;
 size_t sdpcm_size=18+ethernet_len,aligned=(sdpcm_size+3u)&~(size_t)3u;
 if(aligned<=capacity && sdpcm_size<=UINT16_MAX){
  out.checks|=PM_FV_SDPCM_CHECKED;
  if(pm_fv_le16(p)==sdpcm_size && pm_fv_le16(p+2)==(uint16_t)~sdpcm_size &&
     p[4]==sequence && p[5]==2 && p[6]==0 && p[7]==14 &&
     p[8]==0 && p[9]==0 && p[10]==0 && p[11]==0 &&
     p[14]==0x20 && p[15]==0 && p[16]==0 && p[17]==0)
   out.checks|=PM_FV_SDPCM_OK;
 }
 size_t at=12;uint16_t type=pm_fv_be16(eth+at);
 for(unsigned tags=0;tags<2 && (type==0x8100 || type==0x88a8);tags++){
  if(ethernet_len-at<6)return out;
  at+=4;type=pm_fv_be16(eth+at);
 }
 if(type!=0x0800 || ethernet_len-at<22)return out;
 at+=2;const uint8_t *ip=eth+at;size_t avail=ethernet_len-at;
 size_t ihl=(size_t)(ip[0]&15u)*4u,total=pm_fv_be16(ip+2);
 if((ip[0]>>4)!=4 || ihl<20 || ihl>60 || total<ihl+20 || total>avail ||
    ip[9]!=6 || (pm_fv_be16(ip+6)&0x3fffu))return out;
 out.checks|=PM_FV_IP_CHECKED;
 if(pm_fv_fold(pm_fv_sum(0,ip,ihl))==0xffffu)out.checks|=PM_FV_IP_OK;
 const uint8_t *tcp=ip+ihl;size_t tcp_len=total-ihl,thl=(size_t)(tcp[12]>>4)*4u;
 if(thl<20 || thl>60 || tcp_len<thl)return out;
 out.checks|=PM_FV_TCP_CHECKED;
 uint32_t sum=pm_fv_sum(0,ip+12,8);
 sum+=6u+(uint32_t)tcp_len;
 sum=pm_fv_sum(sum,tcp,tcp_len);
 if(pm_fv_fold(sum)==0xffffu)out.checks|=PM_FV_TCP_OK;
 return out;
}
