#include "switch_feedback.h"
#include <assert.h>
#include <stdio.h>
static pm_token tok(uint64_t epoch,uint64_t frame){return (pm_token){1,2,epoch,3,frame};}
int main(void){
 pm_switch_feedback s;pm_switch_reset(&s);
 uint64_t now=UINT64_C(6000000000);pm_switch_sent(&s,tok(10,20),now,true);
 assert(!pm_switch_due(&s,now-1));assert(!pm_switch_due(&s,now+199999));
 assert(pm_switch_due(&s,now+200000));assert(pm_switch_due(&s,now+200001));
 assert(pm_switch_request_hint(&s,now+200000));assert(!pm_switch_request_hint(&s,now+999999));
 // Only a complete, new image in the current scope can suppress the hint.
 pm_switch_reset(&s);pm_switch_sent(&s,tok(10,20),now,true);
 pm_switch_receive(&s,tok(10,21));assert(!pm_switch_received(&s));
 pm_token wrong=tok(11,21);wrong.session_epoch++;pm_switch_receive(&s,wrong);
 assert(!pm_switch_received(&s));wrong=tok(11,21);wrong.screen_owner_epoch++;
 pm_switch_receive(&s,wrong);assert(!pm_switch_received(&s));
 pm_switch_receive(&s,tok(11,20));assert(!pm_switch_received(&s));
 pm_switch_receive(&s,tok(11,21));assert(pm_switch_received(&s));
 assert(!pm_switch_due(&s,now+200000));assert(!pm_switch_complete(&s,tok(10,21)));
 assert(pm_switch_complete(&s,tok(11,21))&&!s.waiting);
 // A second input has its own deadline and cannot finish on the first image.
 pm_switch_reset(&s);pm_switch_sent(&s,tok(1,1),now,true);
 pm_switch_sent(&s,tok(1,1),now+150000,true);
 assert(!pm_switch_due(&s,now+200000));assert(pm_switch_due(&s,now+350000));
 pm_switch_receive(&s,tok(2,2));assert(!pm_switch_received(&s));
 assert(!pm_switch_reply(&s,2,true)&&s.minimum_epoch==3);
 assert(!pm_switch_received(&s));assert(!pm_switch_reply(&s,3,true));
 pm_switch_receive(&s,tok(3,3));assert(pm_switch_received(&s));
 assert(pm_switch_complete(&s,tok(3,3)));
 // Mixed navigation + no-op SUB: frame can precede both replies. The last
 // no-op lowers the provisional floor without discarding that complete frame.
 pm_switch_reset(&s);pm_switch_sent(&s,tok(1,1),now,true);
 pm_switch_sent(&s,tok(1,1),now+1,true);pm_switch_receive(&s,tok(2,2));
 assert(!pm_switch_reply(&s,2,true)&&!pm_switch_received(&s));
 assert(!pm_switch_reply(&s,2,true)&&pm_switch_received(&s));
 assert(pm_switch_complete(&s,tok(2,2)));
 // No-image reply cancels; a late reply for an already drawn request does not
 // consume a later request's response or declare it a no-op.
 pm_switch_reset(&s);pm_switch_sent(&s,tok(1,1),now,true);
 assert(pm_switch_reply(&s,1,true)&&!s.waiting);
 pm_switch_sent(&s,tok(1,1),now,true);pm_switch_receive(&s,tok(2,2));
 assert(pm_switch_complete(&s,tok(2,2)));pm_switch_sent(&s,tok(2,2),now+1,true);
 assert(!pm_switch_reply(&s,2,true)&&s.waiting&&s.minimum_epoch==3);
 assert(!pm_switch_reply(&s,3,true));pm_switch_receive(&s,tok(3,3));
 assert(pm_switch_received(&s));
 // Image priority cancels an outstanding request, not just the adapter. If
 // its bytes disappear, the original deadline can request again; a new input
 // instead uses its own deadline. Old frames cannot consume that request.
 pm_switch_reset(&s);pm_switch_sent(&s,tok(1,1),now,true);
 assert(pm_switch_request_hint(&s,now+200000));
 pm_switch_receive(&s,tok(1,2));assert(!pm_switch_cancel_hint_for_image(&s)&&s.hint_requested);
 pm_switch_receive(&s,tok(2,3));assert(pm_switch_cancel_hint_for_image(&s)&&!s.hint_requested);
 assert(!pm_switch_request_hint(&s,now+300000));pm_switch_forget(&s,tok(2,3));
 assert(pm_switch_request_hint(&s,now+300000));
 pm_switch_receive(&s,tok(2,4));assert(pm_switch_cancel_hint_for_image(&s));
 pm_switch_sent(&s,tok(2,1),now+400000,true);
 assert(!pm_switch_request_hint(&s,now+599999));assert(pm_switch_request_hint(&s,now+600000));
 // Explicit power and a new local owner/session cancel any prior intent.
 pm_switch_sent(&s,tok(3,3),now+2,false);assert(!s.waiting);
 pm_switch_sent(&s,tok(3,3),now+3,true);wrong=tok(1,1);wrong.session_epoch++;
 pm_switch_sent(&s,wrong,now+4,true);assert(s.sent==1&&s.replied==0&&s.minimum_epoch==2);
 pm_switch_reset(&s);assert(!pm_switch_due(&s,UINT64_MAX));
 puts("switch feedback model: 64-bit 199999/200000/200001us, scope, rapid inputs, frame-before-reply, no-op, power PASS");
}
