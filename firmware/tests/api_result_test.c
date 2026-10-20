// Real production handler, assembly, ACK and display lifecycle, with established IO doubles.
#define PM_CONTROL_TEST_ENTRY established_control_cases
#include "control_reply_test.c"
static pm_api_result request(unsigned id,uint64_t offset,unsigned n,bool bad,unsigned session){
 char token[400];token_json(token,sizeof token,id,1,session);
 size_t k=(size_t)snprintf(json,sizeof json,"{\"type\":\"frame_chunk\",\"data\":{%s,\"offset\":%llu,\"bytes\":[",token,(unsigned long long)offset);
 for(unsigned i=0;i<n;i++){json[k++]=bad && i+1==n?'1':'0';if(i+1<n)json[k++]=',';}
 memcpy(json+k,"]}}",4);return api_frame(13,json);
}
static pm_api_result all_chunks(unsigned id,bool bad){
 pm_api_result result=PM_API_REJECTED;
 for(unsigned at=0;at<PM_IMAGE_BYTES;at+=256){unsigned n=PM_IMAGE_BYTES-at;if(n>256)n=256;
  result=request(id,at,n,bad && at+n==PM_IMAGE_BYTES,1);
  if(at+n<PM_IMAGE_BYTES)assert(result==PM_API_PENDING);
 }
 return result;
}
int main(void){
 assert(established_control_cases()==0);
 setup();unsigned id=++serial;start(id,1,1,"normal");
 assert(request(id,1,1,false,1)==PM_API_REJECTED);
 assert(!assembly.active && !frame_receipts && !frame_renders);
 setup();id=++serial;start(id,1,1,"normal");
 assert(request(id,UINT64_C(1)<<32,1,false,1)==PM_API_REJECTED);
 assert(!assembly.active && !frame_receipts && !frame_renders);
 setup();id=++serial;start(id,1,1,"normal");
 assert(request(id,0,1,false,2)==PM_API_REJECTED);
 assert(assembly.active && !assembly.used && !frame_receipts && !frame_renders);
 assert(request(id,0,1,false,1)==PM_API_PENDING && assembly.used==1);
 setup();id=++serial;start(id,1,1,"normal");
 assert(all_chunks(id,true)==PM_API_REJECTED);
 assert(!assembly.active && !frame_receipts && !frame_renders);
 setup();id=++serial;start(id,1,1,"normal");
#if PM_DISPLAY_TYPE == PM_DISPLAY_EINK
 assert(all_chunks(id,false)==PM_API_PENDING);
 assert(drawing && frame_receipts==1 && !frame_renders && !pending_render_ack);
 panel_result=true;poll_display();assert(!drawing && pending_render_ack);
#else
 assert(all_chunks(id,false)==PM_API_COMPLETED);
 assert(frame_receipts==1 && !frame_renders && pending_render_ack);
#endif
 assert(send_pending_render_ack() && frame_renders==1);
 setup();id=++serial;start(id,1,1,"normal");net_send_rejected=true;
 assert(all_chunks(id,false)==PM_API_ERROR);
 assert(!frame_receipts && !frame_renders);net_send_rejected=false;
 setup();id=++serial;start(id,1,1,"normal");display_frame_rejected=true;
 assert(all_chunks(id,false)==PM_API_ERROR);
 assert(frame_receipts==1 && !frame_renders);display_frame_rejected=false;
#if PM_DISPLAY_TYPE == PM_DISPLAY_LCD
 setup();checking_frame=false;id=++serial;stream_start(id,1);
 assert(all_chunks(id,false)==PM_API_PENDING && video.ready);
 assert(!frame_receipts && !frame_renders);poll_video();assert(video.rendered==1);
#endif
 puts("PASS actual API results: invalid/u64 offset, stale token, CRC, partial/pending, draw completion, ACK/draw failure");
 return 0;
}
