#pragma once
#include "radio_query.h"
/* RAM ownership, not a claim that the SDK checked CDC status. Never replace a
 * captured baseline with the experimental value after an uncertain write. */
typedef struct { pm_interference_candidate baseline;bool held,stale; } pm_radio_comparison;
typedef enum { PM_RADIO_TRY3=1,PM_RADIO_RESTORE=2 } pm_radio_change_op;
typedef enum { PM_CHANGE_REJECTED,PM_CHANGE_NO_BASELINE,PM_CHANGE_STALE,
 PM_CHANGE_BASE3,PM_CHANGE_BASE_CHANGED,PM_CHANGE_ALREADY3,PM_CHANGE_WRITE,
 PM_CHANGE_RETURNED } pm_radio_change_phase;
typedef struct {
 pm_radio_change_op op;pm_radio_change_phase phase;
 pm_interference_candidate target,baseline;pm_radio_read read;
 bool wrote;int write_result;
} pm_radio_change;
static inline void pm_radio_comparison_invalidate(pm_radio_comparison *s){if(s->held)s->stale=true;}
static inline pm_radio_change pm_radio_change_rejected(pm_radio_change_op op,pm_radio_read read){
 return (pm_radio_change){.op=op,.phase=PM_CHANGE_REJECTED,.read=read};
}
static inline pm_radio_change pm_radio_change_plan(pm_radio_comparison *s,pm_radio_change_op op,pm_radio_read before){
 pm_radio_change out=pm_radio_change_rejected(op,before);out.baseline=s->baseline;
 if(op!=PM_RADIO_TRY3&&op!=PM_RADIO_RESTORE)return out;
 if(s->stale){out.phase=PM_CHANGE_STALE;return out;}
 if(op==PM_RADIO_RESTORE){
  if(!s->held){out.phase=PM_CHANGE_NO_BASELINE;return out;}
  out.target=s->baseline;out.phase=PM_CHANGE_WRITE;return out;
 }
 pm_interference_candidate candidate;uint32_t raw,base;
 pm_interference_candidate_from_raw(3,&out.target);
 if(!pm_radio_transport_candidate(before,&candidate)||!pm_interference_candidate_value(candidate,&raw))return out;
 if(!s->held){
  if(raw==3){out.phase=PM_CHANGE_BASE3;return out;}
  s->baseline=candidate;s->held=true;out.baseline=candidate;
 }else{
  if(raw==3){out.phase=PM_CHANGE_ALREADY3;return out;}
  if(!pm_interference_candidate_value(s->baseline,&base)||raw!=base){out.phase=PM_CHANGE_BASE_CHANGED;return out;}
 }
 out.phase=PM_CHANGE_WRITE;return out;
}
static inline pm_radio_change pm_radio_change_finish(pm_radio_change plan,int rc,pm_radio_read after){
 plan.wrote=true;plan.write_result=rc;plan.read=after;plan.phase=PM_CHANGE_RETURNED;return plan;
}
static inline bool pm_radio_change_match(pm_radio_change r){
 pm_interference_candidate candidate;uint32_t got,want;
 return pm_radio_transport_candidate(r.read,&candidate)&&pm_interference_candidate_value(candidate,&got)&&
  pm_interference_candidate_value(r.target,&want)&&got==want;
}
static inline const char *pm_radio_change_status(pm_radio_change r){
 switch(r.phase){
 case PM_CHANGE_REJECTED:return pm_radio_status_name(r.read.status);
 case PM_CHANGE_NO_BASELINE:return "no_base";
 case PM_CHANGE_STALE:return "stale";
 case PM_CHANGE_BASE3:return "base_is_3";
 case PM_CHANGE_BASE_CHANGED:return "base_changed";
 case PM_CHANGE_ALREADY3:return "already_3";
 case PM_CHANGE_RETURNED:
  if(r.write_result)return "werr";
  if(r.read.status==PM_RADIO_SDK_ERROR)return "rerr";
  if(r.read.status!=PM_RADIO_TRANSPORT_RETURNED)return "unknown";
  return pm_radio_change_match(r)?"returned":"mismatch";
 default:return "invalid";
 }
}
static inline const char *pm_radio_change_cdc(pm_radio_change r){
 return r.wrote||r.read.status==PM_RADIO_TRANSPORT_RETURNED||r.read.status==PM_RADIO_SDK_ERROR||
  r.read.status==PM_RADIO_UNKNOWN?"unverified":"not_read";
}
typedef pm_radio_change (*pm_radio_changer)(pm_radio_change_op op);
_Static_assert(sizeof(pm_radio_comparison)<=4,"baseline metadata only");
