#include <assert.h>
#include <stdio.h>
#include "observation_json.h"
int main(void){
 uint64_t op=0;
 assert(pm_observation_json_cause("{\"data\":{\"version\":99,\"s\":\"quoted \\\" key\",\"a\":[{},1,true,null]},\"observation\":{\"operation\":2,\"boot\":4660,\"version\":1}}",4660,&op)&&op==2);
 assert(pm_observation_json_cause("{\"observation\":{\"version\":1,\"boot\":4660,\"operation\":18446744073709551615}}",4660,&op)&&op==UINT64_MAX);
 const char *bad[]={
  "{\"data\":{\"version\":1,\"boot\":4660,\"operation\":2}}",
  "{\"data\":{\"observation\":{\"version\":1,\"boot\":4660,\"operation\":2}}}",
  "{\"observation\":{\"version\":1,\"version\":99,\"boot\":4660,\"operation\":2}}",
  "{\"observation\":{\"version\":1,\"boot\":4660,\"boot\":4660,\"operation\":2}}",
  "{\"observation\":{\"version\":1,\"boot\":4660,\"\\u0062oot\":4660,\"operation\":2}}",
  "{\"observation\":{\"version\":1,\"boot\":4660,\"operation\":2,\"operation\":2}}",
  "{\"observation\":{\"version\":1,\"boot\":4660,\"operation\":2},\"observation\":{\"version\":1,\"boot\":4660,\"operation\":2}}",
  "{\"observation\":{\"version\":1,\"boot\":4660,\"operation\":2,\"extra\":1}}",
  "{\"observation\":{\"version\":1,\"boot\":4660,\"operation\":18446744073709551616}}",
  "{\"observation\":{\"version\":1,\"boot\":4660,\"operation\":2.0}}",
  "{\"observation\":{\"version\":1,\"boot\":4660,\"operation\":02}}",
  "{\"observation\":{\"version\":1,\"boot\":4660,\"operation\":0}}",
  "{\"observation\":{\"version\":1,\"boot\":9,\"operation\":2}}",
  "{\"observation\":{\"version\":1,\"boot\":4660,\"operation\":2,}}",
  "{\"observation\":{\"version\":1,\"boot\":4660,\"operation\":2},}",
  "{\"observation\":null}","{\"observation\":[]}","{\"observation\":{\"version\":1}}"
 };
 for(unsigned i=0;i<sizeof bad/sizeof *bad;i++)assert(!pm_observation_json_cause(bad[i],4660,&op));
 puts("PASS scoped unique metadata, escaped duplicate, u64/range/type and malformed JSON");return 0;
}
