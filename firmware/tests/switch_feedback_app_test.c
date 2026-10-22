#define PM_CONTROL_TEST_ENTRY existing_control_cases
#include "control_reply_test.c"
static void begin(void){setup();clock_ms=6000000;}
static void tick(unsigned ms){clock_ms+=ms;poll_switch_feedback();}
static void receive_image(unsigned id,unsigned epoch){
 start(id,epoch,1,"normal");
 for(unsigned at=0;at<PM_IMAGE_BYTES;at+=256){unsigned n=PM_IMAGE_BYTES-at;if(n>256)n=256;native_chunk(id,epoch,1,at,n);}
}
static void finish_draw(void){
#if PM_DISPLAY_TYPE == PM_DISPLAY_EINK
 if(drawing){panel_result=true;poll_display();}
#endif
 if(pending_render_ack)assert(send_pending_render_ack());
}
int main(void){
 // SUB and navigation share one accepted-input clock and never draw on send.
 begin();button(0,false);assert(input_sends==1&&switching.waiting&&!feedback_shows);
 tick(199);assert(!feedback_shows);tick(1);assert(feedback_shows==1);
 tick(1000);assert(feedback_shows==1);reply(10,1,1,1,"normal",false);
 assert(!switching.waiting&&feedback_restores==1);tick(1000);assert(feedback_shows==1);
 // A CRC-complete image just before the deadline avoids the local screen.
 begin();button(3,false);tick(198);receive_image(101,2);finish_draw();
 assert(frame_receipts==1&&frame_renders==1&&!switching.waiting&&!feedback_shows);
 tick(200);assert(!feedback_shows);
 // Header and partial/CRC-invalid payload do not count as arrival.
 begin();button(1,false);start(102,2,1,"normal");chunk(102,2,1,0,256);
 tick(200);assert(feedback_shows==1&&switching.waiting);
 uint8_t bad=1;receive_chunk(assembly.token,256,&bad,1);
 for(unsigned at=257;at<PM_IMAGE_BYTES;at+=256){unsigned n=PM_IMAGE_BYTES-at;if(n>256)n=256;native_chunk(102,2,1,at,n);}
 assert(!pm_switch_received(&switching)&&!frame_receipts&&!frame_renders);
 receive_image(103,2);finish_draw();assert(!switching.waiting&&frame_renders==1&&feedback_shows==1);
 // Repeated input: old frame arrives first but cannot be shown over a newer
 // request. FrameStart-first ordering and a following no-op release it safely.
 begin();button(3,false);tick(100);button(0,false);tick(100);assert(!feedback_shows);
 receive_image(104,2);assert(frame_receipts==1&&!frame_renders&&switching.waiting);
 reply(10,2,1,1,"normal",false);poll_switch_feedback();assert(!frame_renders);
 reply(17,2,1,1,"normal",false);poll_switch_feedback();
#if PM_DISPLAY_TYPE == PM_DISPLAY_EINK
 (void)dispatch_pending_eink();
#endif
 finish_draw();assert(frame_renders==1&&!switching.waiting&&!feedback_shows);
 // A remembered complete image cannot outlive its assembly bytes. A next
 // FrameStart overwrites the provisional same-epoch candidate before no-op.
 begin();receive_image(111,1);finish_draw();button(0,false);tick(200);
 receive_image(112,1);assert(switching.received_valid&&frame_renders==1);
 start(113,1,1,"normal");reply(10,1,1,1,"normal",false);poll_switch_feedback();
 assert(!switching.waiting&&feedback_restores==1&&frame_renders==1);
 // Newest target must win when both requests change the image generation.
 begin();button(3,false);button(3,false);receive_image(105,2);tick(201);
 assert(feedback_shows==1&&!frame_renders);reply(10,2,1,1,"normal",false);
 reply(10,3,1,1,"normal",false);receive_image(106,3);finish_draw();
 assert(frame_renders==1&&last_rendered.frame_id==106&&!switching.waiting);
 // Stale incarnation/session response cannot dismiss the wait; local failure,
 // owner replacement and reconnect all cancel it through existing recovery.
 begin();button(0,false);reply(17,1,2,1,"normal",false);reply(17,1,1,2,"normal",false);
 tick(200);assert(feedback_shows==1&&switching.waiting);retry();assert(!switching.waiting&&state==RETRY);
 begin();net_send_rejected=true;button(0,false);tick(300);assert(!feedback_shows&&!switching.waiting);
 begin();button(3,false);tick(200);display_frame_rejected=true;receive_image(110,2);assert(state==RETRY&&!switching.waiting);
 begin();button(0,false);local("SERVER","CONNECTING");tick(300);assert(!feedback_shows&&!switching.waiting);
 begin();button(3,false);button(2,false);tick(300);assert(!feedback_shows&&!switching.waiting);
#if PM_DISPLAY_TYPE == PM_DISPLAY_LCD
 // Continued frames from an old video cannot consume a SUB wait. A normal
 // no-op reply re-enables the same video without showing the hint.
 begin();checking_frame=false;stream_start(107,1);stream_finish(107);poll_video();assert(video.rendered==1);
 button(0,false);stream_start(108,2);stream_finish(108);tick(200);poll_video();
 assert(video.rendered==1&&switching.waiting&&feedback_shows==1);
 reply(10,1,1,1,"normal",false);clock_ms+=200;poll_video();assert(video.rendered==2&&!switching.waiting);
 // Valid pixels retained across EOF are restored before ownership is invalidated.
 begin();receive_image(109,1);finish_draw();button(0,false);tick(200);
 unsigned before=feedback_restores;retry_with_image(true);assert(feedback_restores==before+1&&!switching.waiting);
#endif
 printf("production switch feedback %s: SUB/UP/DOWN, deadline/CRC, deferred old frames, no-op, rapid input, failure/reconnect/video PASS\n",PM_DISPLAY_CAPABILITY);
}
