#include "core.h"
#include <assert.h>
#include <stdio.h>
static void layout(const pm_button_binding *bindings,unsigned count){
 for(unsigned i=0;i<count;i++){
  pm_button b={0};pm_operation op={0};bool held=false;
  assert(!pm_button_binding_step(&b,&bindings[i],true,100,1800,false,&op,&held));
  bool edge=pm_button_binding_step(&b,&bindings[i],true,135,1800,false,&op,&held);
  assert(edge==bindings[i].on_press);
  if(edge)assert(op.value==bindings[i].short_operation.value&&!held);
  assert(!pm_button_binding_step(&b,&bindings[i],true,136,1800,false,&op,&held));
  assert(!pm_button_binding_step(&b,&bindings[i],false,200,1800,false,&op,&held));
  bool release=pm_button_binding_step(&b,&bindings[i],false,235,1800,false,&op,&held);
  assert(release==!bindings[i].on_press);
  if(release)assert(op.value==bindings[i].short_operation.value&&!held);
  assert(!pm_button_binding_step(&b,&bindings[i],false,300,1800,false,&op,&held));
 }
}
int main(void){
 const pm_button_binding one[]={{{PM_OP_UP},{0},true,false,false}};
 const pm_button_binding three[]={{{PM_OP_SUB},{0},true,false,false},{{PM_OP_DOWN},{PM_OP_POWER},false,false,true},{{PM_OP_UP},{0},false,false,false}};
 const pm_button_binding five[]={three[2],one[0],three[0],three[1],{{PM_OP_SUB},{0},true,false,false}};
 layout(one,1);layout(three,3);layout(five,5);
 for(unsigned count=0;count<=4;count+=2){
  for(unsigned index=count;index<6;index++){
   pm_button b={0},before=b;assert(!pm_button_binding_at(count,index));
   assert(pm_button_input_step(&b,count,index,true,100,1800,false)==-1);assert(!memcmp(&b,&before,sizeof b));
  }
 }
 pm_button b={0};pm_operation op={0};bool held;
 assert(!pm_button_binding_step(&b,&three[1],true,0,1800,true,&op,&held));
 assert(!pm_button_binding_step(&b,&three[1],true,35,1800,true,&op,&held));
 assert(pm_button_binding_step(&b,&three[1],true,1835,1500,true,&op,&held)&&op.value==PM_OP_POWER&&held);
 assert(!pm_button_binding_step(&b,&three[1],false,1900,1500,false,&op,&held));
 assert(!pm_button_binding_step(&b,&three[1],false,1935,1500,false,&op,&held));
 puts("GPIO binding data: arbitrary 1/3/5 layouts, invalid indices, stable release/hold and no duplicate events PASS");
}
