use std::collections::HashSet;
use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let src = env::current_dir().unwrap().join("spdk");

    build_from_source();

    // Tell cargo to tell rustc to link the system shared library.
    println!("cargo:rustc-link-lib=spdk_fat");
    println!("cargo:rustc-link-lib=aio");
    println!("cargo:rustc-link-lib=numa");
    println!("cargo:rustc-link-lib=uuid");
    println!("cargo:rustc-link-lib=crypto");
    println!("cargo:rustc-link-lib=stdc++");
    println!("cargo:rustc-link-lib=ssl");
    println!(
        "cargo:rustc-link-search=native={}",
        env::var("OUT_DIR").unwrap()
    );

    // Tell cargo to invalidate the built crate whenever the wrapper changes
    println!("cargo:rerun-if-changed=wrapper.h");

    let ignored_macros = IgnoreMacros(
        vec![
            "FP_INFINITE".into(),
            "FP_NAN".into(),
            "FP_NORMAL".into(),
            "FP_SUBNORMAL".into(),
            "FP_ZERO".into(),
            // "IPPORT_RESERVED".into(),
        ]
        .into_iter()
        .collect(),
    );

    // The bindgen::Builder is the main entry point
    // to bindgen, and lets you build up options for
    // the resulting bindings.
    let bindings = bindgen::Builder::default()
        .clang_arg(format!("-I{}", src.join("build/include").display()))
        // The input header we would like to generate bindings for.
        .header("wrapper.h")
        .parse_callbacks(Box::new(ignored_macros))
        // Tell cargo to invalidate the built crate whenever any of the
        // included header files changed.
        // .parse_callbacks(Box::new(bindgen::CargoCallbacks))
        .blocklist_item("IPPORT_.*")
        // XXX: workaround for 'error[E0588]: packed type cannot transitively contain a `#[repr(align)]` type'
        .blocklist_type("spdk_nvme_tcp_rsp")
        .blocklist_type("spdk_nvme_tcp_cmd")
        .blocklist_type("spdk_nvmf_fabric_prop_get_rsp")
        .blocklist_type("spdk_nvmf_fabric_connect_rsp")
        .blocklist_type("spdk_nvmf_fabric_connect_cmd")
        .blocklist_type("spdk_nvmf_fabric_auth_send_cmd")
        .blocklist_type("spdk_nvmf_fabric_auth_recv_cmd")
        .blocklist_type("spdk_nvme_health_information_page")
        .blocklist_type("spdk_nvme_ctrlr_data")
        .blocklist_function("spdk_nvme_ctrlr_get_data")
        .opaque_type("spdk_nvme_sgl_descriptor")
        // Finish the builder and generate the bindings.
        .generate()
        // Unwrap the Result and panic on failure.
        .expect("Unable to generate bindings");

    // Write the bindings to the $OUT_DIR/bindings.rs file.
    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Couldn't write bindings!");
}

#[derive(Debug)]
struct IgnoreMacros(HashSet<String>);

impl bindgen::callbacks::ParseCallbacks for IgnoreMacros {
    fn will_parse_macro(&self, name: &str) -> bindgen::callbacks::MacroParsingBehavior {
        if self.0.contains(name) {
            bindgen::callbacks::MacroParsingBehavior::Ignore
        } else {
            bindgen::callbacks::MacroParsingBehavior::Default
        }
    }
}

fn build_from_source() {
    let src = env::current_dir().unwrap().join("spdk");
    let dst = PathBuf::from(env::var("OUT_DIR").unwrap()).join("libspdk_fat.so");

    // Return if the outputs exist.
    // if dst.exists() {
    //     return;
    // }

    // Initialize git submodule if necessary.
    if !Path::new("spdk/.git").exists() {
        let _ = Command::new("git")
            .args(&["submodule", "update", "--init", "--recursive"])
            .status();
    }

    // configure
    let status = Command::new("bash")
        .current_dir(&src)
        .arg("./configure")
        .arg("--without-isal")
        .status()
        .expect("failed to configure");
    assert!(status.success(), "failed to configure: {}", status);

    // make
    #[cfg(target_arch = "aarch64")]
    let status = Command::new("make")
        .current_dir(&src) 
        .arg("DPDKBUILD_FLAGS=\"-Dplatform=generic\"")
        .arg(&format!("-j{}", env::var("NUM_JOBS").unwrap()))
        .status()
        .expect("failed to make");  

    #[cfg(not(target_arch = "aarch64"))]
    let status = Command::new("make")
        .current_dir(&src)  
        .arg(&format!("-j{}", env::var("NUM_JOBS").unwrap()))
        .status()
        .expect("failed to make");

    assert!(status.success(), "failed to make: {}", status);

    // link all shared libraries into 'libspdk_fat.so'
    let mut cc = Command::new("cc");
    cc.arg("-shared")
        .arg("-o")
        .arg(dst)
        .arg("-laio")
        .arg("-lnuma")
        .arg("-luuid")
        .arg("-lcrypto")
        .arg("-Wl,--whole-archive");

    let spdks = std::fs::read_dir(src.join("build/lib")).unwrap();
    let dpdks = std::fs::read_dir(src.join("dpdk/build/lib")).unwrap();
    for e in spdks.chain(dpdks) {
        let entry = e.expect("failed to read directory entry");
        let name = entry.file_name();
        let name = name.to_str().unwrap();
        if name == "libspdk_ut_mock.a" {
            continue;
        }
        if name.starts_with("lib") && name.ends_with(".a") {
            cc.arg(entry.path());
        }
    }
    cc.arg("-Wl,--no-whole-archive");
    let status = cc.status().expect("failed to generate libspdk_fat.so");
    assert!(
        status.success(),
        "failed to generate libspdk_fat.so: {}",
        status
    );
}
