use std::{
    env,
    path::{Path, PathBuf},
    process::Command,
};

fn run(mut cmd: Command) {
    let display = format!("{cmd:?}");
    let status = cmd
        .status()
        .unwrap_or_else(|err| panic!("failed to run {display}: {err}"));
    if !status.success() {
        panic!("{display} failed with status {status}");
    }
}

fn main() {
    println!("cargo:rerun-if-env-changed=ZKVM_C_BENCH");
    println!("cargo:rerun-if-env-changed=ZKVM_BENCH_SIZE");
    println!("cargo:rerun-if-env-changed=ZK_CFLAGS");
    println!("cargo:rerun-if-env-changed=ZK_PASSES");
    println!("cargo:rerun-if-env-changed=ZK_LLVMFLAGS");
    println!("cargo:rerun-if-env-changed=ZK_CLANG_PATH");
    println!("cargo:rerun-if-env-changed=ZK_OPT_PATH");
    println!("cargo:rerun-if-env-changed=ZK_LLC_PATH");
    println!("cargo:rerun-if-env-changed=ZK_LOOP_RISK");
    println!("cargo:rerun-if-env-changed=ZK_LOOP_RISK_PLUGIN");
    println!("cargo:rerun-if-env-changed=ZK_LOOP_RISK_PLUGIN_HASH");
    println!("cargo:rerun-if-env-changed=ZK_LOOP_RISK_PASS");
    println!("cargo:rerun-if-env-changed=ZK_LOOP_RISK_JSONL");
    println!("cargo:rerun-if-env-changed=ZK_LOOP_RISK_SUMMARY");
    println!("cargo:rerun-if-env-changed=ZK_SOURCE_PAGING");

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let root = manifest_dir.ancestors().nth(2).unwrap().to_path_buf();
    let bench = env::var("ZKVM_C_BENCH").unwrap_or_else(|_| "packed_memory_scan".to_string());
    let source = root.join("benchmarks").join(&bench).join("main.c");
    if !source.exists() {
        panic!("unknown C benchmark {bench}: {}", source.display());
    }
    println!("cargo:rerun-if-changed={}", source.display());
    println!(
        "cargo:rerun-if-changed={}",
        root.join("include/zkvm.h").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        root.join("include/zkvm_arena.h").display()
    );

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let ll = out_dir.join(format!("{bench}.ll"));
    let opt_ll = out_dir.join(format!("{bench}.opt.ll"));
    let analysis_ll = out_dir.join(format!("{bench}.analysis.ll"));
    let obj = out_dir.join(format!("{bench}.o"));
    let archive = out_dir.join("libzkvm_c_bench.a");

    let clang = env::var("ZK_CLANG_PATH").unwrap_or_else(|_| "clang".to_string());
    let opt = env::var("ZK_OPT_PATH").unwrap_or_else(|_| "opt".to_string());
    let llc = env::var("ZK_LLC_PATH").unwrap_or_else(|_| "llc".to_string());
    let cflags = env::var("ZK_CFLAGS").unwrap_or_else(|_| "-O0".to_string());
    let passes = env::var("ZK_PASSES").unwrap_or_else(|_| "lower-atomic".to_string());
    let llvmflags = env::var("ZK_LLVMFLAGS").unwrap_or_default();
    let bench_size = env::var("ZKVM_BENCH_SIZE").unwrap_or_else(|_| "small".to_string());
    let source_paging = env::var("ZK_SOURCE_PAGING").as_deref() == Ok("1");

    let size_define = match bench_size.as_str() {
        "tiny" => Some(("ZKVM_BENCH_N", "8")),
        "small" => Some(("ZKVM_BENCH_N", "16")),
        "medium" => Some(("ZKVM_BENCH_N", "24")),
        "large" => Some(("ZKVM_BENCH_N", "256")),
        "xlarge" => Some(("ZKVM_BENCH_N", "1024")),
        "huge" => Some(("ZKVM_BENCH_N", "4096")),
        _ => None,
    };
    let page_define = match bench_size.as_str() {
        "tiny" => Some(("ZKVM_BENCH_PAGES", "16")),
        "small" => Some(("ZKVM_BENCH_PAGES", "64")),
        "medium" => Some(("ZKVM_BENCH_PAGES", "128")),
        "large" => Some(("ZKVM_BENCH_PAGES", "256")),
        "xlarge" => Some(("ZKVM_BENCH_PAGES", "1024")),
        "huge" => Some(("ZKVM_BENCH_PAGES", "4096")),
        _ => None,
    };

    let mut clang_cmd = Command::new(&clang);
    clang_cmd
        .arg("--target=riscv32-unknown-none")
        .arg("-march=rv32im")
        .arg("-mabi=ilp32")
        .arg("-S")
        .arg("-emit-llvm")
        .arg("-Xclang")
        .arg("-disable-O0-optnone")
        .arg("-I")
        .arg(root.join("include"))
        .arg("-Wno-incompatible-library-redeclaration");
    if source_paging {
        clang_cmd
            .arg("-gline-tables-only")
            .arg("-fdebug-compilation-dir")
            .arg(&root);
    }
    for flag in cflags.split_whitespace() {
        clang_cmd.arg(flag);
    }
    if let Some((name, value)) = size_define {
        clang_cmd.arg(format!("-D{name}={value}"));
    }
    if let Some((name, value)) = page_define {
        clang_cmd.arg(format!("-D{name}={value}"));
    }
    clang_cmd.arg(&source).arg("-o").arg(&ll);
    run(clang_cmd);

    if passes == "none" {
        std::fs::copy(&ll, &opt_ll).expect("failed to copy unoptimized LLVM IR");
    } else {
        let mut opt_cmd = Command::new(&opt);
        opt_cmd.arg("-S");
        if let Ok(plugin) = env::var("ZK_LOOP_RISK_PLUGIN") {
            if !plugin.is_empty() {
                opt_cmd.arg("-load-pass-plugin").arg(plugin);
            }
        }
        opt_cmd.arg(format!("-passes={passes}"));
        for flag in llvmflags.split_whitespace() {
            opt_cmd.arg(flag);
        }
        opt_cmd.arg(&ll).arg("-o").arg(&opt_ll);
        run(opt_cmd);
    }

    let codegen_ll = if env::var("ZK_LOOP_RISK").as_deref() == Ok("1") {
        let plugin = env::var("ZK_LOOP_RISK_PLUGIN")
            .expect("ZK_LOOP_RISK_PLUGIN is required when ZK_LOOP_RISK=1");
        let pass = env::var("ZK_LOOP_RISK_PASS").unwrap_or_else(|_| "zk-loop-risk".to_string());
        let jsonl = env::var("ZK_LOOP_RISK_JSONL")
            .expect("ZK_LOOP_RISK_JSONL is required when ZK_LOOP_RISK=1");
        let summary = env::var("ZK_LOOP_RISK_SUMMARY")
            .expect("ZK_LOOP_RISK_SUMMARY is required when ZK_LOOP_RISK=1");

        let _ = std::fs::remove_file(&jsonl);
        let _ = std::fs::remove_file(&summary);

        let mut analysis_cmd = Command::new(&opt);
        analysis_cmd
            .arg("-S")
            .arg("-load-pass-plugin")
            .arg(plugin)
            .arg(format!("-passes=function({pass}),zk-loop-risk-summary"))
            .arg(format!("-zk-loop-risk-jsonl={jsonl}"))
            .arg(format!("-zk-loop-risk-summary={summary}"))
            .arg(&opt_ll)
            .arg("-o")
            .arg(&analysis_ll);
        run(analysis_cmd);
        analysis_ll.as_path()
    } else {
        opt_ll.as_path()
    };

    let mut llc_cmd = Command::new(&llc);
    llc_cmd
        .arg("-mtriple=riscv32-unknown-none")
        .arg("-mattr=+m")
        .arg("-filetype=obj")
        .arg(codegen_ll)
        .arg("-o")
        .arg(&obj);
    run(llc_cmd);

    let ar = find_ar(&root);
    let mut ar_cmd = Command::new(ar);
    ar_cmd.arg("rcs").arg(&archive).arg(&obj);
    run(ar_cmd);

    println!("cargo:rustc-link-search=native={}", out_dir.display());
    println!("cargo:rustc-link-lib=static=zkvm_c_bench");
}

fn find_ar(root: &Path) -> String {
    if let Ok(ar) = env::var("ZK_AR_PATH") {
        return ar;
    }
    let candidates = [
        root.join("../old/toolchains/guest-rust/zk_sched/real/lib/rustlib/x86_64-unknown-linux-gnu/bin/llvm-ar"),
        PathBuf::from("llvm-ar"),
        PathBuf::from("ar"),
    ];
    for candidate in candidates {
        let value = candidate.to_string_lossy().to_string();
        if Command::new(&value).arg("--version").status().is_ok() {
            return value;
        }
    }
    "ar".to_string()
}
