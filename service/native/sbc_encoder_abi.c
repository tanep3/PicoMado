/* Project-owned MIT-licensed ABI; the vendored encoder retains Apache-2.0. */
#include <stdint.h>
#include <stdlib.h>
#include "sbc_encoder.h"

/* This ABI has no independent concurrency guarantee. Its Rust owner serializes
 * init/encode across all contexts because the encoder uses global scratch. */
typedef struct {
    SBC_ENC_PARAMS params;
    uint16_t frame_bytes;
} pm_sbc_encoder;

void *pm_sbc_encoder_new(uint32_t rate, uint8_t channels, uint8_t blocks,
                         uint8_t subbands, uint8_t mode, uint8_t allocation,
                         uint8_t bitpool) {
    if (rate != 48000 || channels != 2 || blocks != 16 || subbands != 8 ||
        (mode != SBC_STEREO && mode != SBC_JOINT_STEREO) || allocation > 1 ||
        bitpool < 2 || bitpool > 53) return NULL;
    pm_sbc_encoder *encoder = calloc(1, sizeof(*encoder));
    if (!encoder) return NULL;
    SBC_ENC_PARAMS *p = &encoder->params;
    p->s16SamplingFreq = SBC_sf48000;
    p->s16NumOfChannels = channels;
    p->s16NumOfBlocks = blocks;
    p->s16NumOfSubBands = subbands;
    p->s16ChannelMode = mode;
    p->s16AllocationMethod = allocation;
    p->s16BitPool = bitpool;
    encoder->frame_bytes = (uint16_t)(12 + (mode == SBC_JOINT_STEREO) + 2 * bitpool);
    SBC_Encoder_Init(p);
    return encoder;
}

/* One exact 128-frame, two-channel s16le block. Explicit byte conversion avoids
 * alignment, endian and Rust/C integer-layout assumptions. */
int pm_sbc_encoder_encode(void *opaque, const uint8_t *pcm, uint32_t pcm_bytes,
                          uint8_t *output, uint32_t output_capacity) {
    pm_sbc_encoder *encoder = opaque;
    if (!encoder || !pcm || pcm_bytes != 512 || !output ||
        output_capacity < encoder->frame_bytes) return -1;
    int16_t samples[256];
    for (unsigned i = 0; i < 256; ++i) {
        uint16_t value = (uint16_t)pcm[2*i] | ((uint16_t)pcm[2*i+1] << 8);
        int32_t signed_value = value < 32768 ? value : (int32_t)value - 65536;
        samples[i] = (int16_t)signed_value;
    }
    encoder->params.ps16PcmBuffer = samples;
    encoder->params.pu8Packet = output;
    SBC_Encoder(&encoder->params);
    /* Do not retain caller/stack pointers after this synchronous call. */
    encoder->params.ps16PcmBuffer = NULL;
    encoder->params.ps16NextPcmBuffer = NULL;
    encoder->params.pu8Packet = NULL;
    encoder->params.pu8NextPacket = NULL;
    if (encoder->params.u16PacketLength != encoder->frame_bytes) return -2;
    return encoder->params.u16PacketLength;
}

void pm_sbc_encoder_free(void *opaque) { free(opaque); }
