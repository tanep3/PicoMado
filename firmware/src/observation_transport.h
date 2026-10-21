#pragma once
#include <stdbool.h>
#include <stdint.h>
#include <string.h>
#define PM_OB_TX_SLOTS 8u
#define PM_OB_RX_SLOTS 8u
typedef struct {uint64_t copy,notify;uint32_t first,end;bool used;} pm_ob_tx_range;
typedef struct {pm_ob_tx_range slots[PM_OB_TX_SLOTS];unsigned next;uint32_t last_end;bool copied,wrapped;} pm_ob_tx_map;
static inline bool pm_ob_overlap(uint32_t first,uint32_t end,uint32_t a,uint32_t b){
    /* Every represented interval is shorter than 2^31; subtraction handles wrap. */
    return first!=end && a!=b && (int32_t)(end-a)>0 && (int32_t)(b-first)>0;
}
static inline void pm_ob_tx_publish(pm_ob_tx_map *m,uint32_t first,uint32_t end,uint64_t copy,uint64_t notify){
    if((m->copied && first!=m->last_end) || end<first){m->wrapped=true;memset(m->slots,0,sizeof m->slots);}
    m->last_end=end;m->copied=true;
    if(m->wrapped || !copy || !notify || first==end || end-first>=UINT32_C(0x80000000))return;
    m->slots[m->next]=(pm_ob_tx_range){copy,notify,first,end,true};m->next=(m->next+1)%PM_OB_TX_SLOTS;
}
static inline pm_ob_tx_range pm_ob_tx_find(const pm_ob_tx_map *m,uint32_t first,uint32_t end){
    pm_ob_tx_range result={0};unsigned matches=0;
    if(m->wrapped || first>=end || end-first>=UINT32_C(0x80000000))return result;
    for(unsigned i=0;i<PM_OB_TX_SLOTS;i++)if(m->slots[i].used && pm_ob_overlap(first,end,m->slots[i].first,m->slots[i].end)){
        result=m->slots[i];matches++;
    }
    return matches==1 && result.first<=first && end<=result.end?result:(pm_ob_tx_range){0};
}
typedef struct {uint32_t first,end;uint64_t operation;} pm_ob_rx_range;
typedef struct {pm_ob_rx_range slots[PM_OB_RX_SLOTS];unsigned next;} pm_ob_rx_map;
static inline void pm_ob_rx_publish(pm_ob_rx_map *m,uint32_t first,uint32_t end,uint64_t op){
    if(!op || first>=end)return;
    m->slots[m->next]=(pm_ob_rx_range){first,end,op};m->next=(m->next+1)%PM_OB_RX_SLOTS;
}
static inline uint64_t pm_ob_rx_find(const pm_ob_rx_map *m,uint32_t first,uint32_t end){
    uint64_t operation=0;unsigned matches=0;
    for(unsigned i=0;i<PM_OB_RX_SLOTS;i++)if(m->slots[i].operation && m->slots[i].first<=first && end<=m->slots[i].end){operation=m->slots[i].operation;matches++;}
    return matches==1?operation:0;
}
/* Map contiguous raw spans into the bounded app workspace; overflow is unknown. */
typedef struct {uint32_t first,length;} pm_ob_app_span;
typedef struct {pm_ob_app_span spans[PM_OB_RX_SLOTS];unsigned count;bool unknown;} pm_ob_app_map;
static inline void pm_ob_app_append(pm_ob_app_map *m,uint32_t first,uint32_t length){
    if(!length || m->unknown)return;
    if(m->count && m->spans[m->count-1].first+m->spans[m->count-1].length==first){m->spans[m->count-1].length+=length;return;}
    if(m->count==PM_OB_RX_SLOTS){m->unknown=true;return;}
    m->spans[m->count++]=(pm_ob_app_span){first,length};
}
static inline bool pm_ob_app_take(pm_ob_app_map *m,uint32_t length,uint32_t *first,uint32_t *end){
    if(m->unknown || !m->count)return false;
    *first=m->spans[0].first;uint32_t remaining=length;
    while(remaining && m->count){
        pm_ob_app_span *span=&m->spans[0];uint32_t n=remaining<span->length?remaining:span->length;
        *end=span->first+n;span->first+=n;span->length-=n;remaining-=n;
        if(!span->length){m->count--;memmove(m->spans,m->spans+1,m->count*sizeof m->spans[0]);}
    }
    return !remaining;
}
