#pragma once
#include <stdbool.h>
#include <stdint.h>
#include <string.h>
/* BT-AUDIO-20261107, spec2896889. Pure bounded setup recovery, no SDK IO. */
typedef enum {PM_BR_OFF,PM_BR_OBSERVE,PM_BR_RELEASE,PM_BR_COOLDOWN} pm_bt_recovery_phase;
typedef enum {PM_BR_NONE,PM_BR_DISCONNECT,PM_BR_CONNECT,PM_BR_FAIL} pm_bt_recovery_action;
typedef struct {
 uint64_t generation;uint8_t peer[6];unsigned retries;
 uint32_t started,since,release_started;uint16_t observed_cid;
 pm_bt_recovery_phase phase;
} pm_bt_recovery;
static inline void pm_bt_recovery_cancel(pm_bt_recovery *r){r->phase=PM_BR_OFF;r->observed_cid=0;}
static inline void pm_bt_recovery_start(pm_bt_recovery *r,uint64_t gen,const uint8_t peer[6],uint32_t now){
 memset(r,0,sizeof *r);r->generation=gen;memcpy(r->peer,peer,6);r->started=r->since=now;r->phase=PM_BR_OBSERVE;
}
static inline bool pm_bt_recovery_reserve(pm_bt_recovery *r){
 if(r->phase==PM_BR_OFF||r->retries>=2)return false;
 ++r->retries;return true;
}
static inline pm_bt_recovery_action pm_bt_recovery_step(pm_bt_recovery *r,uint64_t gen,
 const uint8_t peer[6],bool wanted,bool connected,uint16_t cid,uint16_t avrcp_cid,
 bool sdk_owner,bool discover_wait,uint32_t now){
 if(r->phase==PM_BR_OFF)return PM_BR_NONE;
 if(!wanted||gen!=r->generation||memcmp(peer,r->peer,6)){pm_bt_recovery_cancel(r);return PM_BR_NONE;}
 if((uint32_t)(now-r->started)>=25000){pm_bt_recovery_cancel(r);return PM_BR_FAIL;}
 if(connected&&r->phase==PM_BR_OBSERVE){pm_bt_recovery_cancel(r);return PM_BR_NONE;}
 switch(r->phase){
 case PM_BR_OBSERVE:
  if(!discover_wait||!cid){r->observed_cid=0;r->since=now;return PM_BR_NONE;}
  if(r->observed_cid!=cid){r->observed_cid=cid;r->since=now;return PM_BR_NONE;}
  if((uint32_t)(now-r->since)<5000)return PM_BR_NONE;
  if(!pm_bt_recovery_reserve(r)){pm_bt_recovery_cancel(r);return PM_BR_FAIL;}
  r->phase=PM_BR_RELEASE;r->release_started=now;return PM_BR_DISCONNECT;
 case PM_BR_RELEASE:
 case PM_BR_COOLDOWN:
  if((uint32_t)(now-r->release_started)>=3000){pm_bt_recovery_cancel(r);return PM_BR_FAIL;}
  if(cid||avrcp_cid||sdk_owner){r->phase=PM_BR_RELEASE;return PM_BR_NONE;}
  if(r->phase==PM_BR_RELEASE){r->phase=PM_BR_COOLDOWN;r->since=now;return PM_BR_NONE;}
  if((uint32_t)(now-r->since)<500)return PM_BR_NONE;
  r->phase=PM_BR_OBSERVE;r->observed_cid=0;r->since=now;return PM_BR_CONNECT;
 default:return PM_BR_NONE;
 }
}
