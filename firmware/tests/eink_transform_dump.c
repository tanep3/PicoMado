#include <stdio.h>
#include "eink_transform.h"
int main(void){
    uint8_t source[3813],panel[4000];
    if(fread(source,1,sizeof source,stdin)!=sizeof source)return 2;
    pm_eink_pack_landscape(source,panel);
    return fwrite(panel,1,sizeof panel,stdout)==sizeof panel?0:3;
}
