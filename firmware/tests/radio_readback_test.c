// Reuse the real main/owner setup helpers without renaming firmware main.
#define PM_COMPARE_TEST_ENTRY radio_comparison_regressions
#include "radio_compare_test.c"
int main(void){
 pm_interference_readback obs;pm_interference_candidate setting;uint32_t value;
 assert(!pm_interference_candidate_from_raw(0x84,&setting)); // Never SET raw status.
 for(uint32_t mode=0;mode<=4;mode++)for(unsigned active=0;active<2;active++){
  uint32_t raw=mode|(active?0x80u:0);
  assert(pm_interference_readback_from_raw(raw,&obs));
  assert(obs.auto_active==(active!=0)&&pm_interference_candidate_value(obs.setting,&value)&&value==mode);
 }
 const uint32_t unknown[]={5,0x85,0x88,0x44,0x104,0x184,0x84000000,UINT32_MAX};
 for(unsigned i=0;i<sizeof unknown/sizeof unknown[0];i++){
  fresh(unknown[i]);change("RADIO INTERFERENCE TRY3 22 1");
  assert(!radio_write_calls&&!radio_comparison.held);fact("s=unknown");
 }
 fresh(0x84);radio_sdk_result=-110;change("RADIO INTERFERENCE TRY3 22 1");
 fact("s=sdk_error");assert(!radio_write_calls&&!radio_comparison.held);
 // GET preserves raw and publishes the separate reported activity flag.
 fresh(0x84);text("RADIO INTERFERENCE GET 22");service();
 fact("raw=00000084 candidate=4 auto_active=1 cdc_status=unverified\n");
 // Setting 3, with an active status bit in the subsequent GET, is still a
 // setting-candidate match. Restore writes 4, never the original raw 0x84.
 fresh(0x84);radio_after_override=true;radio_after_result=0;radio_after_value=0x83;
 change("RADIO INTERFERENCE TRY3 22 1");
 assert(radio_write_calls==1&&radio_last_written==3&&radio_comparison.held);
 assert(radio_comparison.baseline.auto_active);
 assert(pm_interference_candidate_value(radio_comparison.baseline.setting,&value)&&value==4);
 fact("s=returned w=0 r=0 x=00000083 t=3 base=4 m=1 cdc_status=unverified");
 radio_after_value=0x84;change("RADIO INTERFERENCE RESTORE 22 2");
 assert(radio_write_calls==2&&radio_last_written==4);
 fact("x=00000084 t=4 base=4 m=1 cdc_status=unverified");
 // Activity may change independently: restoring the same setting with no
 // AUTO_ACTIVE flag is also a mode-candidate match, not a status restoration.
 radio_after_value=0x83;change("RADIO INTERFERENCE TRY3 22 3");
 radio_after_value=4;change("RADIO INTERFERENCE RESTORE 22 4");
 assert(radio_write_calls==4&&radio_last_written==4);
 fact("x=00000004 t=4 base=4 m=1 cdc_status=unverified");
 puts("radio readback: raw/mode/activity types, 0x84 baseline -> SET3/GET0x83 -> SET4/GET0x84 or4, strict unknown/error rejection PASS");
 return 0;
}
