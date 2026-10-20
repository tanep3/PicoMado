
#define PM_CONTROL_TEST_ENTRY pm_control_existing_tests
#include "control_reply_test.c"
static void complete_without_ack(unsigned id){
 start(id,1,1,"normal");
 for(unsigned at=0;at<PM_IMAGE_BYTES;at+=256){unsigned n=PM_IMAGE_BYTES-at;if(n>256)n=256;chunk(id,1,1,at,n);}
#if PM_DISPLAY_TYPE == PM_DISPLAY_EINK
 if(drawing){panel_result=true;poll_display();}
#endif
}
int main(void){
#if PM_ACK_PATH_PROBE
 pm_ack_probe_init_boot(1);pm_ack_probe_command("AP2 ARM 1 1");pm_ack_probe_button(1,1,42,1,false);
#endif
 setup();complete_without_ack(1001);
 assert(frame_receipts==1&&frame_renders==0&&pending_render_ack&&pending_render_token.frame_id==1001);
 complete_without_ack(1002);
 assert(frame_receipts==2&&frame_renders==0&&pending_render_token.frame_id==1002);
#if PM_ACK_PATH_PROBE
 bool replaced=false;pm_ack_probe_record record;
 for(unsigned i=0;pm_ack_probe_read(i,&record);i++)if(record.event==PM_AP_PENDING_REPLACED&&record.frame==1001&&record.a==1002)replaced=true;
 assert(replaced);
#endif
 assert(send_pending_render_ack()&&frame_renders==1&&!pending_render_ack);
 // Valid local input is sent without waiting for the old rendered ACK.
 setup();complete_without_ack(1003);button(1,false);
 assert(await_selection&&pending_render_ack);assert(send_pending_render_ack());
 assert(frame_renders==1); // epoch is unchanged until the server's new generation.
 setup();complete_without_ack(1004);display_epoch++;
 assert(send_pending_render_ack()&&!pending_render_ack&&!frame_renders);
 setup();complete_without_ack(1005);network_stage=PM_NET_DRAINED;
 assert(send_pending_render_ack()&&!pending_render_ack&&!frame_renders);
#if PM_ACK_PATH_PROBE
 pm_ack_probe_init_boot(1);pm_ack_probe_command("AP2 ARM 1 1");pm_ack_probe_button(1,1,42,1,false);setup();start(1006,1,1,"normal");chunk(1006,1,1,1,256);
 bool rejected=false,crc_complete=false;
 for(unsigned i=0;pm_ack_probe_read(i,&record);i++)if(record.event==PM_AP_FRAME_CHECK){rejected|=record.a==1;crc_complete|=record.a==0;}
 assert(rejected&&!crc_complete&&!assembly.active);
#if PM_DISPLAY_TYPE == PM_DISPLAY_LCD
 pm_ack_probe_init_boot(1);pm_ack_probe_command("AP2 ARM 1 1");pm_ack_probe_button(1,1,42,1,false);setup();stream_start(1007,1);native_chunk(1007,1,1,1,256);
 for(unsigned i=0;pm_ack_probe_read(i,&record);i++)assert(record.event!=PM_AP_FRAME_CHECK);
#endif
#endif
 puts("PASS production pending ACK: one-slot overwrite if completion reenters before drain; unchanged-epoch button does not suppress; new epoch/Close suppresses");
 puts("LIMIT: forced back-to-back full-frame calls bypass main-loop budgets/server dispatch; this is slot behavior, not proof of a reachable production overwrite");
 return 0;
}
