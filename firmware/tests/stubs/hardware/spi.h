#pragma once
#include <stddef.h>
#include <stdint.h>
#include <stdbool.h>
typedef struct { unsigned unused; } spi_inst_t;
extern spi_inst_t *spi0;
extern spi_inst_t *spi1;
#define SPI_CPOL_0 0
#define SPI_CPHA_0 0
#define SPI_MSB_FIRST 0
unsigned spi_init(spi_inst_t *spi, unsigned baud);
unsigned spi_set_baudrate(spi_inst_t *spi, unsigned baud);
unsigned spi_get_baudrate(const spi_inst_t *spi);
bool spi_is_busy(const spi_inst_t *spi);
void spi_set_format(spi_inst_t *spi, int bits, int cpol, int cpha, int order);
int spi_write_blocking(spi_inst_t *spi, const uint8_t *bytes, size_t length);
