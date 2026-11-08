#include <assert.h>
#include <stdio.h>
#include "bluetooth_recovery.h"
#include "bluetooth_event_queue.h"
static uint8_t peer[6]={1,2,3,4,5,6},other[6]={1,2,3,4,5,7};
static pm_bt_recovery r;
static pm_bt_recovery_action step(uint32_t t,uint16_t cid,uint16_t rc,bool owner,bool stall){return pm_bt_recovery_step(&r,1,peer,true,false,cid,rc,owner,stall,t);}
static void start(void){pm_bt_recovery_start(&r,1,peer,0);assert(step(0,4,8,true,true)==PM_BR_NONE);}
static void stall(void){assert(step(5000,4,8,true,true)==PM_BR_DISCONNECT);}
static void test_normal(void){start();assert(step(4999,4,8,true,true)==PM_BR_NONE);stall();
 assert(step(5001,0,8,true,false)==PM_BR_NONE);assert(step(5002,0,0,true,false)==PM_BR_NONE);
 assert(step(5100,0,0,false,false)==PM_BR_NONE);assert(step(5599,0,0,false,false)==PM_BR_NONE);
 assert(step(5600,0,0,false,false)==PM_BR_CONNECT);assert(r.retries==1);
 assert(step(5700,5,0,true,true)==PM_BR_NONE);assert(step(10700,5,0,true,true)==PM_BR_DISCONNECT);
 assert(step(10701,0,0,false,false)==PM_BR_NONE);assert(step(11201,0,0,false,false)==PM_BR_CONNECT);
 assert(step(11300,6,0,true,true)==PM_BR_NONE);assert(step(16300,6,0,true,true)==PM_BR_FAIL);
 assert(step(20000,6,0,true,true)==PM_BR_NONE);
}
static void test_progress_and_owner(void){start();assert(step(4000,4,8,true,false)==PM_BR_NONE);assert(step(4500,4,8,true,true)==PM_BR_NONE);assert(step(9499,4,8,true,true)==PM_BR_NONE);assert(step(9500,4,8,true,true)==PM_BR_DISCONNECT);
 assert(step(9501,4,0,true,false)==PM_BR_NONE);assert(step(9600,0,0,true,false)==PM_BR_NONE);assert(step(12500,0,0,true,false)==PM_BR_FAIL);
 start();stall();assert(step(5001,0,0,false,false)==PM_BR_NONE);assert(step(5300,0,0,true,false)==PM_BR_NONE);assert(step(5400,0,0,false,false)==PM_BR_NONE);assert(step(5899,0,0,false,false)==PM_BR_NONE);assert(step(5900,0,0,false,false)==PM_BR_CONNECT);
 start();assert(step(4999,5,8,true,true)==PM_BR_NONE);assert(step(5000,5,8,true,true)==PM_BR_NONE);
}
static void test_budget(void){start();assert(pm_bt_recovery_reserve(&r));stall();assert(r.retries==2);assert(step(5001,0,0,false,false)==PM_BR_NONE);assert(step(5501,0,0,false,false)==PM_BR_CONNECT);assert(!pm_bt_recovery_reserve(&r));
 start();stall();assert(pm_bt_recovery_reserve(&r));assert(!pm_bt_recovery_reserve(&r));
}
static void test_cancel_timeout(void){
 for(unsigned phase=PM_BR_OBSERVE;phase<=PM_BR_COOLDOWN;phase++){
  for(unsigned reason=0;reason<3;reason++){start();r.phase=(pm_bt_recovery_phase)phase;
   assert(pm_bt_recovery_step(&r,reason==0?2:1,reason==1?other:peer,reason!=2,reason==3,4,8,true,true,500)==PM_BR_NONE);assert(r.phase==PM_BR_OFF);assert(step(20000,4,8,true,true)==PM_BR_NONE);}
  start();r.phase=(pm_bt_recovery_phase)phase;r.since=r.release_started=24999;
  assert(step(24999,4,8,true,false)==PM_BR_NONE);assert(step(25000,4,8,true,true)==PM_BR_FAIL);assert(step(25001,4,8,true,true)==PM_BR_NONE);
 }
 pm_bt_recovery_start(&r,1,peer,UINT32_MAX-100);assert(pm_bt_recovery_step(&r,1,peer,true,false,4,0,true,true,UINT32_MAX-100)==PM_BR_NONE);
 assert(step(4898,4,0,true,true)==PM_BR_NONE);assert(step(4899,4,0,true,true)==PM_BR_DISCONNECT);
}
static void test_success_at_deadline(void){
 for(unsigned phase=PM_BR_RELEASE;phase<=PM_BR_COOLDOWN;phase++){
  start();r.phase=(pm_bt_recovery_phase)phase;r.release_started=5000;
  assert(pm_bt_recovery_step(&r,1,peer,true,true,4,8,true,false,5001)==PM_BR_NONE);
  assert(r.phase==PM_BR_RELEASE);
 }

 start();assert(pm_bt_recovery_step(&r,1,peer,true,true,4,8,true,false,25000)==PM_BR_FAIL);
 start();assert(pm_bt_recovery_step(&r,1,peer,true,true,4,8,true,false,24999)==PM_BR_NONE);assert(r.phase==PM_BR_OFF);
}
static void test_queue(void){pm_bt_event_queue q={0};for(unsigned i=0;i<100;i++)pm_bt_event_enqueue(&q,(pm_bt_event){.kind=PM_BT_LINK_DIAGNOSTIC});assert(q.count==4);
 pm_bt_event_enqueue(&q,(pm_bt_event){.kind=PM_BT_CONNECTED});pm_bt_event_enqueue(&q,(pm_bt_event){.kind=PM_BT_ERROR});pm_bt_event_enqueue(&q,(pm_bt_event){.kind=PM_BT_STOPPED});assert(q.count==7);
 pm_bt_event e;unsigned terminals=0;while(pm_bt_event_dequeue(&q,&e))terminals+=pm_bt_event_terminal(e.kind);assert(terminals==3);
 for(unsigned i=0;i<8;i++)pm_bt_event_enqueue(&q,(pm_bt_event){.kind=PM_BT_CONNECTED,.generation=i});
 pm_bt_event_enqueue(&q,(pm_bt_event){.kind=PM_BT_ERROR,.generation=9});while(pm_bt_event_dequeue(&q,&e)){}assert(e.kind==PM_BT_ERROR&&e.generation==9);
}
static void test_setup_identity(void){
 pm_bt_setup_progress p={.phase=1};
 start();pm_bt_recovery_progress(&r,&p,0);assert(step(0,4,8,true,true)==PM_BR_NONE);
 p.phase=2;pm_bt_recovery_progress(&r,&p,4999);assert(step(4999,4,8,true,true)==PM_BR_NONE);
 assert(step(5000,4,8,true,true)==PM_BR_NONE);assert(step(9998,4,8,true,true)==PM_BR_NONE);assert(step(9999,4,8,true,true)==PM_BR_DISCONNECT);
#define TEST_FIELD(f) do { start();p=(pm_bt_setup_progress){.phase=2};pm_bt_recovery_progress(&r,&p,0);assert(step(0,4,8,true,true)==PM_BR_NONE);++p.f;pm_bt_recovery_progress(&r,&p,4999);assert(step(4999,4,8,true,true)==PM_BR_NONE);assert(step(5000,4,8,true,true)==PM_BR_NONE);assert(step(9999,4,8,true,true)==PM_BR_DISCONNECT); } while(0);
 PM_BR_PROGRESS_FIELDS(TEST_FIELD)
#undef TEST_FIELD
 start();p=(pm_bt_setup_progress){.phase=2};pm_bt_recovery_progress(&r,&p,0);assert(step(0,4,8,true,true)==PM_BR_NONE);
 p.phase=1;pm_bt_recovery_progress(&r,&p,4999);assert(step(4999,4,8,true,true)==PM_BR_NONE);assert(step(9999,4,8,true,true)==PM_BR_DISCONNECT);
}
int main(void){test_setup_identity();test_normal();test_progress_and_owner();test_budget();test_cancel_timeout();test_queue();test_success_at_deadline();puts("Bluetooth recovery: bounded retry, teardown/SDK owner, cancellation, deadline, queue PASS");}
