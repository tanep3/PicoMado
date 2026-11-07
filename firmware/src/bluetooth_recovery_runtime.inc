/* Actual SDK effect coordinator, shared with host fake-SDK tests. */
static void recovery_poll(btstack_timer_source_t *t){
 (void)t;
 if(recovery.phase==PM_BR_OFF)return;
 uint32_t now=btstack_run_loop_get_time_ms();
 avdtp_connection_t *owner=avdtp_get_connection_for_bd_addr(peer);
 avdtp_connection_t *owned=cid?avdtp_get_connection_for_avdtp_cid(cid):NULL;
 bool stall=owned&&owned->a2dp_source_config_process.state==A2DP_W4_DISCOVER_SEPS&&
     owned->initiator_connection_state==AVDTP_SIGNALING_CONNECTION_INITIATOR_W4_ANSWER;
 bool connected=owned&&owned->a2dp_source_config_process.state==A2DP_STREAMING_OPENED;
 uint16_t previous=recovery.observed_cid;
 pm_bt_recovery_action action=pm_bt_recovery_step(&recovery,generation,peer,wanted&&!stopping,
     connected,cid,remote_cid,owner!=NULL,stall&&!retry_pending,now);
 if(previous!=recovery.observed_cid&&action==PM_BR_NONE)
  emit((pm_bt_event){.kind=PM_BT_LINK_DIAGNOSTIC,.detail1=cid,.detail2=stall,.detail3=4});
 if(action==PM_BR_DISCONNECT){
  retry_cancel();media_stop();
  emit((pm_bt_event){.kind=PM_BT_LINK_DIAGNOSTIC,.detail1=cid,.detail2=(int32_t)recovery.retries,.detail3=5});
  uint8_t result=cid?a2dp_source_disconnect(cid):0;
  if(!result&&remote_cid)result=avrcp_disconnect(remote_cid);
  if(result){retry_cancel();recovery_fail(result);return;}
 }else if(action==PM_BR_CONNECT){
  /* Both release callbacks and peer SDK owner absence were checked above.
   * Do not reuse retry_connect's existing-owner path for stalled DISCOVER. */
  retry_cancel();uint16_t next=0;
  uint8_t result=a2dp_source_establish_stream(peer,&next);
  emit((pm_bt_event){.kind=PM_BT_LINK_DIAGNOSTIC,.status=result,.detail1=next,.detail2=(int32_t)recovery.retries,.detail3=6});
  if(!result)cid=next;
  else if(result==ERROR_CODE_ACL_CONNECTION_ALREADY_EXISTS)retry_schedule();
  else {recovery_fail(result);return;}
 }else if(action==PM_BR_FAIL){retry_cancel();recovery_fail(0xff10);return;}
 if(recovery.phase!=PM_BR_OFF){btstack_run_loop_set_timer(&recovery_timer,250);btstack_run_loop_add_timer(&recovery_timer);}
}
