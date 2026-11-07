#pragma once
#include <stddef.h>
#include <stdint.h>
#include <stdbool.h>
/* One packet targets <=40ms; SBC media header has a four-bit count. */
static inline unsigned pm_bt_packet_frames(unsigned rate,unsigned samples,unsigned bytes,size_t mtu,size_t capacity){
 if(!rate||!samples||!bytes||mtu<2||capacity<2)return 0;
 size_t limit=mtu<capacity?mtu:capacity;
 unsigned n=(unsigned)((limit-1)/bytes),time_limit=rate/25/samples;
 if(n>15)n=15;
 if(n>time_limit)n=time_limit;
 return n;
}
/* Positive delay is paced; a late callback catches up one bounded packet per
 * subsequent timer turn, never an unbounded while-loop. */
static inline int32_t pm_bt_packet_delay(uint32_t start,uint64_t sent,unsigned rate,uint32_t now){
 return (int32_t)(start+(uint32_t)(sent*1000/rate)-now);
}

/* Lower SBC rate reduces packet count on the measured704-byte sink MTU.
 * The chosen bitpool must remain within the peer-negotiated range. */
static inline unsigned pm_bt_choose_bitpool(unsigned minimum,unsigned maximum){
 if(minimum<2||maximum>53||minimum>maximum)return 0;
 unsigned chosen=maximum<32?maximum:32;
 return chosen<minimum?minimum:chosen;
}
/* Exact16blocks,8subbands,2channels SBC byte size, before first encode. */
static inline unsigned pm_bt_sbc_bytes(unsigned bitpool,int joint){return 12u+(joint?1u:0u)+2u*bitpool;}
/* Zero means wait. Only a completely received declared EOF may use a short
 * packet. Every other packet must amortize radio access with the full target. */
static inline unsigned pm_bt_ready_packet(unsigned target,unsigned samples,size_t available,
        bool eof,uint64_t accepted,uint64_t end){
 if(!target||!samples)return 0;
 unsigned n=(unsigned)(available/samples);
 if(n>=target)return target;
 return eof&&accepted==end?n:0;
}
