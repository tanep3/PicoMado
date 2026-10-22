#pragma once
#include "radio_compare.h"
/* Canonical roam_off payload, distinct from interference mode. 0-init invalid. */
typedef struct {uint8_t encoded;} pm_roaming_candidate;
static inline bool pm_roaming_from_raw(uint32_t raw,pm_roaming_candidate *out){
 if(!out||raw>1)return false;
 *out=(pm_roaming_candidate){(uint8_t)(raw+1)};return true;
}
static inline bool pm_roaming_value(pm_roaming_candidate value,uint32_t *out){
 if(!out||value.encoded<1||value.encoded>2)return false;
 *out=(uint32_t)value.encoded-1;return true;
}
static inline pm_radio_read pm_roaming_decode(int rc,uint32_t raw){
 pm_roaming_candidate value;
 return (pm_radio_read){rc?PM_RADIO_SDK_ERROR:(pm_roaming_from_raw(raw,&value)?PM_RADIO_TRANSPORT_RETURNED:PM_RADIO_UNKNOWN),rc,raw};
}
static inline bool pm_roaming_transport(pm_radio_read read,pm_roaming_candidate *out){
 return read.status==PM_RADIO_TRANSPORT_RETURNED&&!read.sdk_result&&pm_roaming_from_raw(read.raw,out);
}
typedef struct {pm_roaming_candidate baseline;bool held,stale;} pm_roaming_comparison;
typedef enum {PM_ROAM_OFF=1,PM_ROAM_RESTORE=2} pm_roaming_op;
typedef struct {pm_roaming_op op;pm_radio_change_phase phase;pm_roaming_candidate target,baseline;
 pm_radio_read read;bool wrote;int write_result;} pm_roaming_change;
static inline pm_roaming_change pm_roaming_rejected(pm_roaming_op op,pm_radio_read read){
 return (pm_roaming_change){.op=op,.phase=PM_CHANGE_REJECTED,.read=read};
}
static inline void pm_roaming_invalidate(pm_roaming_comparison *s){if(s->held)s->stale=true;}
static inline pm_roaming_change pm_roaming_plan(pm_roaming_comparison *s,pm_roaming_op op,pm_radio_read before){
 pm_roaming_change out=pm_roaming_rejected(op,before);out.baseline=s->baseline;
 if(op!=PM_ROAM_OFF&&op!=PM_ROAM_RESTORE)return out;
 pm_roaming_candidate got={0};uint32_t value=0,base=0;
 bool valid=pm_roaming_transport(before,&got)&&pm_roaming_value(got,&value);
 if(s->held)pm_roaming_value(s->baseline,&base);
 /* Explicit recovery restores the captured original even after STA reset;
  * it does not reopen the original comparison interval or clear stale. */
 out.phase=pm_radio_comparison_decide(s->held,s->stale&&op!=PM_ROAM_RESTORE,
      op==PM_ROAM_RESTORE,valid,value,base,1);
 if(out.phase!=PM_CHANGE_WRITE)return out;
 if(op==PM_ROAM_RESTORE)out.target=s->baseline;
 else {pm_roaming_from_raw(1,&out.target);if(!s->held){s->baseline=got;s->held=true;out.baseline=got;}}
 return out;
}
static inline bool pm_roaming_match(pm_roaming_change r){
 pm_roaming_candidate got;uint32_t a,b;
 return pm_roaming_transport(r.read,&got)&&pm_roaming_value(got,&a)&&pm_roaming_value(r.target,&b)&&a==b;
}
static inline const char *pm_roaming_change_status(pm_roaming_change r){
 switch(r.phase){
 case PM_CHANGE_BASE3:return "already_off";
 case PM_CHANGE_ALREADY3:return "already_off";
 case PM_CHANGE_NO_BASELINE:return "no_base";
 case PM_CHANGE_STALE:return "stale";
 case PM_CHANGE_BASE_CHANGED:return "base_changed";
 case PM_CHANGE_REJECTED:return pm_radio_status_name(r.read.status);
 case PM_CHANGE_RETURNED:
  if(r.write_result)return "werr";
  if(r.read.status==PM_RADIO_SDK_ERROR)return "rerr";
  if(r.read.status!=PM_RADIO_TRANSPORT_RETURNED)return "unknown";
  return pm_roaming_match(r)?"returned":"mismatch";
 default:return "invalid";
 }
}
static inline const char *pm_roaming_cdc(pm_roaming_change r){
 return r.wrote||r.read.status==PM_RADIO_TRANSPORT_RETURNED||r.read.status==PM_RADIO_SDK_ERROR||r.read.status==PM_RADIO_UNKNOWN?"unverified":"not_read";
}
typedef pm_roaming_change (*pm_roaming_changer)(pm_roaming_op op);
_Static_assert(sizeof(pm_roaming_comparison)<=4,"baseline metadata only");
