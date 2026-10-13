#include <stdio.h>
#include "pico/stdlib.h"
#include "display.h"
#include "core.h"

int main(void) {
    stdio_init_all();
    pm_display_init();
    const uint pins[]={PM_BUTTON_SUB_PIN,PM_BUTTON_UP_PIN,
                       PM_BUTTON_POWER_PIN,PM_BUTTON_DOWN_PIN};
    pm_button buttons[PM_BUTTON_COUNT]={0};
    for (unsigned i=0;i<PM_BUTTON_COUNT;i++) { gpio_init(pins[i]); gpio_set_dir(pins[i],GPIO_IN); gpio_pull_up(pins[i]); }
    pm_display_status("PICO MADO", "PRESS BUTTON");
    puts("PicoMado selftest: LCD initialized; waiting for buttons");
    const char *names[]={"SUB", "UP", "POWER", "DOWN"};
    while (true) {
        uint32_t now=to_ms_since_boot(get_absolute_time());
        for (unsigned i=0;i<PM_BUTTON_COUNT;i++) {
            if (pm_button_step(&buttons[i],!gpio_get(pins[i]),now)) {
                pm_display_status("BUTTON",names[i]);
                printf("button %u\n",i);
            }
        }
        sleep_ms(5);
    }
}
