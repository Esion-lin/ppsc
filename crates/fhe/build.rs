//! Build script: compile the OpenFHE C ABI wrapper and link prebuilt OpenFHE.

use std::env;
use std::path::PathBuf;

fn find_gpp() -> PathBuf {
    // MSYS2 MinGW64 g++ is the canonical compiler for OpenFHE on Windows.
    let candidates = [
        r"C:\msys64\mingw64\bin\g++.exe",
        r"C:\msys64\ucrt64\bin\g++.exe",
        r"C:\msys64\clang64\bin\g++.exe",
    ];
    for c in candidates {
        if PathBuf::from(c).exists() {
            return PathBuf::from(c);
        }
    }
    PathBuf::from("g++")
}

fn main() {
    let manifest =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set by cargo"));
    println!("cargo:rerun-if-env-changed=OPENFHE_DIR");
    println!("cargo:rerun-if-env-changed=OPENFHE_LIB_DIR");
    let openfhe = env::var_os("OPENFHE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let root = manifest.join("../..");
            let local = root.join("openfhe-development");
            if local.exists() { local } else { root.join("target/openfhe-development") }
        });
    println!("cargo:rerun-if-changed=cpp/capi.cpp");
    println!("cargo:rerun-if-changed=cpp/capi.h");
    let target_os = env::var("CARGO_CFG_TARGET_OS").expect("target os");

    let mut build = cc::Build::new();
    build.cpp(true);
    build.compiler(find_gpp());
    build.flag("-std=gnu++17");
    // Suppress warnings from OpenFHE's own headers (unused parameters etc.).
    build.flag("-w");
    build.opt_level(2);
    build.include(openfhe.join("src/core/include"));
    build.include(openfhe.join("src/pke/include"));
    build.include(openfhe.join("src/binfhe/include"));
    build.include(openfhe.join("third-party/cereal/include"));
    build.include(openfhe.join("build/src/core"));
    build.file("cpp/capi.cpp");
    build.compile("ppsc_fhe_capi");

    let library_dir = env::var_os("OPENFHE_LIB_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| openfhe.join("build/lib"));
    println!("cargo:rustc-link-search=native={}", library_dir.display());
    println!("cargo:rustc-link-lib=static=OPENFHEpke_static");
    println!("cargo:rustc-link-lib=static=OPENFHEcore_static");
    println!("cargo:rustc-link-lib=static=OPENFHEbinfhe_static");
    match target_os.as_str() {
        "windows" => {
            println!("cargo:rustc-link-search=native=C:/msys64/mingw64/lib");
            println!("cargo:rustc-link-lib=dylib=stdc++");
            println!("cargo:rustc-link-lib=dylib=winpthread");
            // MinGW's libmingwex references symbols supplied by ucrtbase/moldname.
            println!("cargo:rustc-link-arg=-Wl,-Bstatic,-lucrtbase,-Bdynamic");
            println!("cargo:rustc-link-arg=-Wl,-Bstatic,-lmoldname,-Bdynamic");
        }
        "macos" => println!("cargo:rustc-link-lib=dylib=c++"),
        _ => {
            println!("cargo:rustc-link-lib=dylib=stdc++");
            println!("cargo:rustc-link-lib=dylib=pthread");
        }
    }
}
