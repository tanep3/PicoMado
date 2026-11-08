// BT-AUDIO-20261107: fixed Apache-2.0 host encoder, without a Pico SDK.
fn main() {
    let vendor = "vendor/bluedroid-sbc";
    let sources = [
        "sbc_analysis.c",
        "sbc_dct.c",
        "sbc_dct_coeffs.c",
        "sbc_enc_bit_alloc_mono.c",
        "sbc_enc_bit_alloc_ste.c",
        "sbc_enc_coeffs.c",
        "sbc_encoder.c",
        "sbc_packing.c",
    ];
    let headers = [
        "sbc_dct.h",
        "sbc_enc_func_declare.h",
        "sbc_encoder.h",
        "sbc_types.h",
    ];
    let mut build = cc::Build::new();
    build
        .include(format!("{vendor}/include"))
        .std("c11")
        .opt_level(2)
        // The fixed-point codec aliases its history buffer and uses wrapping
        // signed arithmetic. Make those assumptions explicit when supported.
        .flag_if_supported("-fno-strict-aliasing")
        .flag_if_supported("-fwrapv");
    for source in sources {
        let path = format!("{vendor}/srce/{source}");
        println!("cargo:rerun-if-changed={path}");
        build.file(path);
    }
    for header in headers {
        println!("cargo:rerun-if-changed={vendor}/include/{header}");
    }
    println!("cargo:rerun-if-changed={vendor}/provenance.json");
    println!("cargo:rerun-if-changed=native/sbc_encoder_abi.c");
    build
        .file("native/sbc_encoder_abi.c")
        .compile("picomado_sbc_encoder");
}
