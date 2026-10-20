// Actual main/API/frame/display flow. Only established hardware/network doubles.
#define PM_CAUSAL_FIXTURE 1
#define PM_CONTROL_TEST_ENTRY established_control_cases
#include "control_reply_test.c"
static void wire(uint8_t kind,const char *body){printf("WIRE %u %s\n",kind,body);}
static unsigned digit(char c){if(c>='0'&&c<='9')return (unsigned)(c-'0');if(c>='a'&&c<='f')return (unsigned)(c-'a'+10);assert(false);return 0;}
static void flush_events(void){
 for(unsigned i=0;i<1024;i++)pm_trace_flush();
 if(trace_used){fwrite(trace_text,1,trace_used,stdout);trace_used=0;trace_text[0]=0;}
 puts("DONE");fflush(stdout);
}
int main(void){
 setup();clock_ms=1000;trace_output=true;wire_observer=wire;
 pm_usb_control_init(0x1234,PM_BUTTON_COUNT);
 char line[40000];puts("READY");fflush(stdout);
 while(fgets(line,sizeof line,stdin)){
  char *nl=strchr(line,'\n');if(nl)*nl=0;
  if(!strncmp(line,"USB ",4)){
   for(char *p=line+4;*p;p++)pm_usb_control_input(*p);
   pm_usb_control_input('\n');pm_usb_control_poll(true);
  }else if(!strncmp(line,"BUTTON ",7))button((unsigned)strtoul(line+7,NULL,10),false);
  else if(!strncmp(line,"ADVANCE ",8))clock_ms+=(uint32_t)strtoul(line+8,NULL,10);
  else if(!strcmp(line,"FAIL DRAW"))display_frame_rejected=true;
  else if(!strcmp(line,"POLL")){
#if PM_DISPLAY_TYPE == PM_DISPLAY_EINK
   panel_result=true;poll_display();
#else
   poll_video();
#endif
   send_pending_render_ack();
  }else if(!strncmp(line,"PACKET ",7)){
   const char *hex=line+7;size_t n=strlen(hex);assert(n%2==0 && n/2<sizeof json);
   for(size_t i=0;i<n/2;i++)json[i]=(char)(digit(hex[2*i])*16+digit(hex[2*i+1]));
   n/=2;assert(n>=6 && (uint8_t)json[4]==1);uint8_t kind=(uint8_t)json[5];
   memmove(json,json+6,n-6);json[n-6]=0;
   if(kind==18)process_frame_bytes((const uint8_t *)json,n-6);else process(kind,json);
  }else assert(false);
  flush_events();
 }
 return 0;
}
