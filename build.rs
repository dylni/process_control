use std::env;
use std::ffi::OsStr;

macro_rules! cfg_var {
    ( $name:ident , $value:ident ) => {
        env::var_os(format!(
            "CARGO_{}_{}",
            stringify!($name),
            stringify!($value).to_uppercase(),
        ))
        .is_some()
    };
}

macro_rules! targets {
    ( $name:ident => $($value:ident),+ ) => {
        env::var_os(concat!("CARGO_CFG_TARGET_", stringify!($name)))
            .as_deref()
            .and_then(OsStr::to_str)
            .map(|values| {
                let values: Vec<_> = values.split(',').collect();
                [$(stringify!($value)),+]
                    .into_iter()
                    .any(|x| values.contains(&x))
            })
            .unwrap_or(false)
    };
}

macro_rules! new_cfg {
    ( $name:ident , $condition:expr , ) => {{
        if $condition {
            println!("cargo:rustc-cfg=process_control_{}", stringify!($name));
        }
    }};
}

#[allow(clippy::nonminimal_bool)]
fn main() {
    new_cfg!(
        memory_limit,
        targets!(OS => android)
            || (targets!(OS => linux) && targets!(ENV => gnu, musl))
            || cfg_var!(CFG, windows),
    );
    new_cfg!(
        waitid,
        !cfg_var!(FEATURE, __unstable_force_missing_waitid)
            && !targets!(OS => espidf, horizon, openbsd, redox, tvos, vxworks),
    );
}
