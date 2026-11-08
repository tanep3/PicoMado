/* Capture decision metadata before cancel clears the owner. No extra event. */
static void recovery_fail(uint16_t status){
 pm_bt_event e={.kind=PM_BT_ERROR,.status=status};
 if(status==0xff10){
  uint32_t elapsed=btstack_run_loop_get_time_ms()-recovery.started;
  e.detail1=recovery.reason;e.detail2=elapsed>INT32_MAX?INT32_MAX:(int32_t)elapsed;
  e.detail3=(int32_t)recovery.retries;
 }
 recovery_cancel();if(!wanted)return;wanted=false;emit(e);
}
/* Scalar SDK snapshot shared by diagnostics and no-progress identity. */
static pm_bt_setup_progress setup_progress(avdtp_connection_t *c){
 pm_bt_setup_progress p={0};if(!c)return p;
 p.connection=c->state;p.initiator=c->initiator_connection_state;p.configuration=c->configuration_state;
 p.initiator_label=c->initiator_transaction_label;p.acceptor_label=c->acceptor_transaction_label;
 p.initiator_signal=c->initiator_signaling_packet.signal_identifier;p.acceptor_signal=c->acceptor_signaling_packet.signal_identifier;
 p.initiator_packet_label=c->initiator_signaling_packet.transaction_label;p.acceptor_packet_label=c->acceptor_signaling_packet.transaction_label;
 avdtp_stream_endpoint_t *e=c->a2dp_source_config_process.local_stream_endpoint;
 if(e){p.endpoint_valid=true;p.endpoint=e->state;p.endpoint_initiator=e->initiator_config_state;p.endpoint_acceptor=e->acceptor_config_state;p.local_seid=avdtp_stream_endpoint_seid(e);p.active_matches=c->active_stream_endpoint==e;}
 if(c->state==AVDTP_SIGNALING_CONNECTION_OPENED){
  if(c->a2dp_source_config_process.state==A2DP_W4_DISCOVER_SEPS&&c->initiator_connection_state==AVDTP_SIGNALING_CONNECTION_INITIATOR_W4_ANSWER)p.phase=1;
  else if(c->a2dp_source_config_process.state==A2DP_W4_SET_CONFIGURATION&&e)p.phase=2;
 }
 return p;
}
/* Actual SDK effect coordinator, shared with host fake-SDK tests. */
static void recovery_poll(btstack_timer_source_t *t){
 (void)t;
 if(recovery.phase==PM_BR_OFF)return;
 uint32_t now=btstack_run_loop_get_time_ms();
 avdtp_connection_t *owner=avdtp_get_connection_for_bd_addr(peer);
 avdtp_connection_t *owned=cid?avdtp_get_connection_for_avdtp_cid(cid):NULL;
 pm_bt_setup_progress progress=setup_progress(owned);
 pm_bt_recovery_progress(&recovery,&progress,now);
 bool stall=progress.phase!=0;
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
