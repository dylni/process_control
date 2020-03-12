use std::env;

const LIB_FILE: &str = "src/externs/libwait_for_process.c";

fn main() {
    if env::var_os("CARGO_CFG_UNIX").is_some() {
        println!("cargo:rerun-if-changed={}", LIB_FILE);

        cc::Build::new().file(LIB_FILE).compile("wait_for_process");
    }
}
