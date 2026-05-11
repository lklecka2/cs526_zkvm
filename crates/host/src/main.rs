use risc0_zkvm::{ExecutorEnv, ExecutorImpl, TraceEvent};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    cell::RefCell,
    collections::{BTreeMap, HashMap, HashSet},
    env, fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    rc::Rc,
    time::Instant,
};

#[derive(Default)]
struct Metrics {
    pc_histogram: HashMap<u32, u64>,
    written_pages: HashSet<u32>,
    source_paging: Option<SourcePaging>,
    registers: [u32; 32],
    current_pc: Option<u32>,
}

#[derive(Serialize)]
struct ExecuteJson {
    benchmark: String,
    profile: String,
    size: String,
    elf_path: String,
    elf_sha256: String,
    committed_i32: Option<i32>,
    executor_wall_ms: u128,
    user_cycles: u64,
    paging_cycles: u64,
    reserved_cycles: u64,
    total_cycles: u64,
    segment_count: usize,
    segment_po2s: Vec<usize>,
    dynamic_instruction_count: u64,
    unique_pcs_executed: usize,
    page_in_count: u64,
    page_out_count: u64,
    unique_pages_written: usize,
    source_paging_enabled: bool,
    synthetic_page_in_count: u64,
    synthetic_page_out_count: u64,
    source_page_hotspots_path: Option<String>,
    segment_model: String,
}

#[derive(Default)]
struct SourcePaging {
    page_ins: HashMap<u32, u64>,
    page_outs: HashMap<u32, u64>,
    code_page_ins: HashMap<u32, u64>,
    load_page_ins: HashMap<u32, u64>,
    store_page_ins: HashMap<u32, u64>,
    store_page_outs: HashMap<u32, u64>,
    loaded_pages: HashSet<u32>,
    dirty_pages: HashSet<u32>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Ord, PartialOrd)]
struct SourceLocation {
    function: String,
    file: String,
    line: u64,
    column: u64,
    external: bool,
}

#[derive(Clone, Default, Serialize)]
struct SourceLineReport {
    file: String,
    line: u64,
    column: u64,
    function: String,
    synthetic_page_ins: u64,
    synthetic_page_outs: u64,
    code_page_ins: u64,
    load_page_ins: u64,
    store_page_ins: u64,
    store_page_outs: u64,
    executed_instructions: u64,
    external: bool,
}

#[derive(Serialize)]
struct SourcePagingSummary {
    enabled: bool,
    segment_model: String,
    total_synthetic_page_ins: u64,
    total_synthetic_page_outs: u64,
    unique_pages_loaded: usize,
    unique_pages_dirtied: usize,
    session_paging_cycles: u64,
    top_hotspots: Vec<SourceLineReport>,
}

fn main() -> Result<(), String> {
    let args = Args::parse(env::args().skip(1).collect())?;
    fs::create_dir_all(&args.out_dir).map_err(|err| err.to_string())?;
    if args.source_paging {
        fs::create_dir_all(&args.analysis_dir).map_err(|err| err.to_string())?;
    }

    let elf = fs::read(&args.elf_path).map_err(|err| err.to_string())?;
    let metrics = Rc::new(RefCell::new(Metrics {
        source_paging: args.source_paging.then(SourcePaging::default),
        ..Metrics::default()
    }));
    let trace_metrics = Rc::clone(&metrics);

    let mut env_builder = ExecutorEnv::builder();
    env_builder
        .stdout(std::io::sink())
        .trace_callback(move |event| {
            let mut metrics = trace_metrics.borrow_mut();
            match event {
                TraceEvent::InstructionStart { pc, insn, .. } => {
                    *metrics.pc_histogram.entry(pc).or_insert(0) += 1;
                    metrics.current_pc = Some(pc);
                    let regs = metrics.registers;
                    if let Some(source_paging) = metrics.source_paging.as_mut() {
                        source_paging.touch_code(pc);
                        if let Some(addr) = decode_load_addr(insn, &regs) {
                            source_paging.touch_load(pc, addr);
                        }
                    }
                }
                TraceEvent::RegisterSet { idx, value } => {
                    if idx < metrics.registers.len() {
                        metrics.registers[idx] = value;
                    }
                }
                TraceEvent::MemorySet { addr, .. } => {
                    metrics.written_pages.insert(addr >> 10);
                    if let (Some(pc), Some(source_paging)) =
                        (metrics.current_pc, metrics.source_paging.as_mut())
                    {
                        source_paging.touch_store(pc, addr);
                    }
                }
                _ => {}
            }
            Ok(())
        });
    if let Some(segment_limit_po2) = args.segment_limit_po2 {
        env_builder.segment_limit_po2(segment_limit_po2);
    }
    let env = env_builder.build().map_err(|err| err.to_string())?;

    let started = Instant::now();
    let mut executor = ExecutorImpl::from_elf(env, &elf).map_err(|err| err.to_string())?;
    let session = executor.run().map_err(|err| err.to_string())?;
    let elapsed = started.elapsed().as_millis();

    let committed_i32 = session
        .journal
        .as_ref()
        .and_then(|journal| journal.decode::<i32>().ok());
    let segment_po2s = session
        .segments
        .iter()
        .filter_map(|segment| segment.resolve().ok().map(|resolved| resolved.po2()))
        .collect::<Vec<_>>();
    let segment_model = if session.segments.len() == 1 {
        "single_segment".to_string()
    } else {
        "multi_segment_lower_bound".to_string()
    };

    let metrics = metrics.borrow();
    let dynamic_instruction_count = metrics.pc_histogram.values().sum();
    let source_report = if let Some(source_paging) = metrics.source_paging.as_ref() {
        Some(write_source_paging_reports(
            &args,
            source_paging,
            &metrics.pc_histogram,
            session.paging_cycles,
            &segment_model,
        )?)
    } else {
        None
    };

    let output = ExecuteJson {
        benchmark: args.benchmark,
        profile: args.profile,
        size: args.size,
        elf_path: args.elf_path.display().to_string(),
        elf_sha256: sha256_hex(&elf),
        committed_i32,
        executor_wall_ms: elapsed,
        user_cycles: session.user_cycles,
        paging_cycles: session.paging_cycles,
        reserved_cycles: session.reserved_cycles,
        total_cycles: session.total_cycles,
        segment_count: session.segments.len(),
        segment_po2s,
        dynamic_instruction_count,
        unique_pcs_executed: metrics.pc_histogram.len(),
        page_in_count: 0,
        page_out_count: 0,
        unique_pages_written: metrics.written_pages.len(),
        source_paging_enabled: args.source_paging,
        synthetic_page_in_count: source_report
            .as_ref()
            .map(|report| report.total_synthetic_page_ins)
            .unwrap_or(0),
        synthetic_page_out_count: source_report
            .as_ref()
            .map(|report| report.total_synthetic_page_outs)
            .unwrap_or(0),
        source_page_hotspots_path: args.source_paging.then(|| {
            args.analysis_dir
                .join("source-page-hotspots.md")
                .display()
                .to_string()
        }),
        segment_model,
    };

    let json = serde_json::to_string_pretty(&output).map_err(|err| err.to_string())?;
    fs::write(args.out_dir.join("execute.json"), format!("{json}\n"))
        .map_err(|err| err.to_string())?;

    println!("{json}");
    Ok(())
}

struct Args {
    benchmark: String,
    profile: String,
    size: String,
    elf_path: PathBuf,
    out_dir: PathBuf,
    analysis_dir: PathBuf,
    source_paging: bool,
    segment_limit_po2: Option<u32>,
}

impl Args {
    fn parse(args: Vec<String>) -> Result<Self, String> {
        let mut benchmark = None;
        let mut profile = None;
        let mut size = None;
        let mut elf_path = None;
        let mut out_dir = None;
        let mut analysis_dir = None;
        let mut source_paging = false;
        let mut segment_limit_po2 = None;
        let mut idx = 0;
        while idx < args.len() {
            let key = &args[idx];
            match key.as_str() {
                "--source-paging" => {
                    source_paging = true;
                    idx += 1;
                    continue;
                }
                "--benchmark" => benchmark = Some(next_value(&args, idx, key)?.to_string()),
                "--profile" => profile = Some(next_value(&args, idx, key)?.to_string()),
                "--size" => size = Some(next_value(&args, idx, key)?.to_string()),
                "--elf" => elf_path = Some(PathBuf::from(next_value(&args, idx, key)?)),
                "--out-dir" => out_dir = Some(PathBuf::from(next_value(&args, idx, key)?)),
                "--analysis-dir" => {
                    analysis_dir = Some(PathBuf::from(next_value(&args, idx, key)?));
                }
                "--segment-limit-po2" => {
                    segment_limit_po2 = Some(
                        next_value(&args, idx, key)?
                            .parse::<u32>()
                            .map_err(|err| format!("invalid --segment-limit-po2: {err}"))?,
                    );
                }
                _ => return Err(format!("unknown argument: {key}")),
            }
            idx += 2;
        }

        let out_dir = out_dir.ok_or("missing --out-dir")?;
        let default_analysis_dir = out_dir
            .parent()
            .and_then(Path::parent)
            .map(|path| path.join("analysis"))
            .unwrap_or_else(|| out_dir.join("analysis"));

        Ok(Self {
            benchmark: benchmark.ok_or("missing --benchmark")?,
            profile: profile.ok_or("missing --profile")?,
            size: size.unwrap_or_else(|| "small".to_string()),
            elf_path: elf_path.ok_or("missing --elf")?,
            out_dir,
            analysis_dir: analysis_dir.unwrap_or(default_analysis_dir),
            source_paging,
            segment_limit_po2,
        })
    }
}

fn next_value<'a>(args: &'a [String], idx: usize, key: &str) -> Result<&'a str, String> {
    args.get(idx + 1)
        .map(String::as_str)
        .ok_or_else(|| format!("missing value for {key}"))
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

impl SourcePaging {
    fn touch_code(&mut self, pc: u32) {
        let loaded = &mut self.loaded_pages;
        touch_loaded_page(loaded, &mut self.page_ins, &mut self.code_page_ins, pc, pc);
    }

    fn touch_load(&mut self, pc: u32, addr: u32) {
        let loaded = &mut self.loaded_pages;
        touch_loaded_page(
            loaded,
            &mut self.page_ins,
            &mut self.load_page_ins,
            pc,
            addr,
        );
    }

    fn touch_store(&mut self, pc: u32, addr: u32) {
        let loaded = &mut self.loaded_pages;
        touch_loaded_page(
            loaded,
            &mut self.page_ins,
            &mut self.store_page_ins,
            pc,
            addr,
        );
        let page = addr >> 10;
        if self.dirty_pages.insert(page) {
            *self.page_outs.entry(pc).or_insert(0) += 1;
            *self.store_page_outs.entry(pc).or_insert(0) += 1;
        }
    }
}

fn touch_loaded_page(
    loaded_pages: &mut HashSet<u32>,
    page_ins: &mut HashMap<u32, u64>,
    kind_counts: &mut HashMap<u32, u64>,
    pc: u32,
    addr: u32,
) {
    let page = addr >> 10;
    if loaded_pages.insert(page) {
        *page_ins.entry(pc).or_insert(0) += 1;
        *kind_counts.entry(pc).or_insert(0) += 1;
    }
}

fn decode_load_addr(insn: u32, registers: &[u32; 32]) -> Option<u32> {
    if insn & 0x7f != 0x03 {
        return None;
    }
    let funct3 = (insn >> 12) & 0x7;
    if !matches!(funct3, 0x0..=0x2 | 0x4 | 0x5) {
        return None;
    }
    let rs1 = ((insn >> 15) & 0x1f) as usize;
    let imm = sign_extend(insn >> 20, 12) as u32;
    Some(registers[rs1].wrapping_add(imm))
}

fn sign_extend(value: u32, bits: u32) -> i32 {
    let shift = 32 - bits;
    ((value << shift) as i32) >> shift
}

fn write_source_paging_reports(
    args: &Args,
    source_paging: &SourcePaging,
    pc_histogram: &HashMap<u32, u64>,
    session_paging_cycles: u64,
    segment_model: &str,
) -> Result<SourcePagingSummary, String> {
    let pc_locations = symbolize_pcs(
        &args.elf_path,
        &args.benchmark,
        pc_histogram.keys().copied(),
    )?;
    let mut by_line = BTreeMap::<SourceLocation, SourceLineReport>::new();

    for (pc, count) in pc_histogram {
        let loc = pc_locations
            .get(pc)
            .cloned()
            .unwrap_or_else(|| SourceLocation {
                function: "<unknown>".to_string(),
                file: "<unknown>".to_string(),
                line: 0,
                column: 0,
                external: true,
            });
        let report = by_line
            .entry(loc.clone())
            .or_insert_with(|| SourceLineReport {
                file: loc.file.clone(),
                line: loc.line,
                column: loc.column,
                function: loc.function.clone(),
                external: loc.external,
                ..SourceLineReport::default()
            });
        report.executed_instructions += count;
        report.synthetic_page_ins += source_paging.page_ins.get(pc).copied().unwrap_or(0);
        report.synthetic_page_outs += source_paging.page_outs.get(pc).copied().unwrap_or(0);
        report.code_page_ins += source_paging.code_page_ins.get(pc).copied().unwrap_or(0);
        report.load_page_ins += source_paging.load_page_ins.get(pc).copied().unwrap_or(0);
        report.store_page_ins += source_paging.store_page_ins.get(pc).copied().unwrap_or(0);
        report.store_page_outs += source_paging.store_page_outs.get(pc).copied().unwrap_or(0);
    }

    let mut reports = by_line.into_values().collect::<Vec<_>>();
    reports.sort_by(|a, b| {
        let a_score = a.synthetic_page_ins + a.synthetic_page_outs;
        let b_score = b.synthetic_page_ins + b.synthetic_page_outs;
        b_score
            .cmp(&a_score)
            .then_with(|| b.executed_instructions.cmp(&a.executed_instructions))
            .then_with(|| a.file.cmp(&b.file))
            .then_with(|| a.line.cmp(&b.line))
    });

    let jsonl = reports
        .iter()
        .map(|report| serde_json::to_string(report).map_err(|err| err.to_string()))
        .collect::<Result<Vec<_>, _>>()?
        .join("\n");
    fs::write(
        args.analysis_dir.join("source-pages.jsonl"),
        if jsonl.is_empty() {
            String::new()
        } else {
            format!("{jsonl}\n")
        },
    )
    .map_err(|err| err.to_string())?;

    let top_hotspots = reports.into_iter().take(20).collect::<Vec<_>>();
    let summary = SourcePagingSummary {
        enabled: true,
        segment_model: segment_model.to_string(),
        total_synthetic_page_ins: source_paging.page_ins.values().sum(),
        total_synthetic_page_outs: source_paging.page_outs.values().sum(),
        unique_pages_loaded: source_paging.loaded_pages.len(),
        unique_pages_dirtied: source_paging.dirty_pages.len(),
        session_paging_cycles,
        top_hotspots,
    };
    fs::write(
        args.analysis_dir.join("source-pages-summary.json"),
        serde_json::to_string_pretty(&summary).map_err(|err| err.to_string())? + "\n",
    )
    .map_err(|err| err.to_string())?;
    fs::write(
        args.analysis_dir.join("source-page-hotspots.md"),
        render_hotspots(args, &summary),
    )
    .map_err(|err| err.to_string())?;
    Ok(summary)
}

fn symbolize_pcs(
    elf_path: &Path,
    benchmark: &str,
    pcs: impl Iterator<Item = u32>,
) -> Result<HashMap<u32, SourceLocation>, String> {
    let mut sorted = pcs.collect::<Vec<_>>();
    sorted.sort_unstable();
    sorted.dedup();
    if sorted.is_empty() {
        return Ok(HashMap::new());
    }

    let mut child = Command::new("llvm-symbolizer")
        .arg(format!("--obj={}", elf_path.display()))
        .arg("--inlining=false")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|err| format!("failed to run llvm-symbolizer: {err}"))?;
    {
        let stdin = child
            .stdin
            .as_mut()
            .ok_or("failed to open llvm-symbolizer stdin")?;
        for pc in &sorted {
            writeln!(stdin, "0x{pc:08x}").map_err(|err| err.to_string())?;
        }
    }
    let output = child
        .wait_with_output()
        .map_err(|err| format!("failed to read llvm-symbolizer output: {err}"))?;
    if !output.status.success() {
        return Err(format!(
            "llvm-symbolizer failed with status {}",
            output.status
        ));
    }

    let text = String::from_utf8_lossy(&output.stdout);
    let mut lines = text.lines();
    let mut out = HashMap::new();
    for pc in &sorted {
        let function = lines.next().unwrap_or("??").to_string();
        let location = lines.next().unwrap_or("??:0:0");
        let _blank = lines.next();
        out.insert(*pc, parse_location(function, location));
    }
    if let Some(cmain_symbol) = find_symbol(elf_path, "cmain")? {
        if let Some(c_object) = find_c_object(elf_path, benchmark) {
            let c_pcs = sorted
                .iter()
                .copied()
                .filter(|pc| {
                    *pc >= cmain_symbol.addr && *pc < cmain_symbol.addr + cmain_symbol.size
                })
                .collect::<Vec<_>>();
            let c_locations = symbolize_c_object_pcs(&c_object, &c_pcs, cmain_symbol.addr)?;
            for (pc, loc) in c_locations {
                out.insert(pc, loc);
            }
        }
    }
    Ok(out)
}

#[derive(Clone, Copy)]
struct SymbolInfo {
    addr: u32,
    size: u32,
}

fn find_symbol(elf_path: &Path, name: &str) -> Result<Option<SymbolInfo>, String> {
    let output = Command::new("llvm-objdump")
        .arg("-t")
        .arg(elf_path)
        .output()
        .map_err(|err| format!("failed to run llvm-objdump: {err}"))?;
    if !output.status.success() {
        return Err(format!("llvm-objdump failed with status {}", output.status));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    for line in text.lines() {
        let fields = line.split_whitespace().collect::<Vec<_>>();
        if fields.last().copied() != Some(name) || fields.len() < 5 {
            continue;
        }
        let addr = u32::from_str_radix(fields[0], 16).unwrap_or(0);
        let size = u32::from_str_radix(fields[4], 16).unwrap_or(0);
        if size > 0 {
            return Ok(Some(SymbolInfo { addr, size }));
        }
    }
    Ok(None)
}

fn find_c_object(elf_path: &Path, benchmark: &str) -> Option<PathBuf> {
    let build_dir = elf_path.parent()?;
    let target_dir = build_dir.join("target");
    let wanted = format!("{benchmark}.o");
    let mut stack = vec![target_dir];
    let mut matches = Vec::new();
    while let Some(dir) = stack.pop() {
        let entries = fs::read_dir(dir).ok()?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.file_name().and_then(|name| name.to_str()) == Some(wanted.as_str()) {
                matches.push(path);
            }
        }
    }
    matches.sort();
    matches.pop()
}

fn symbolize_c_object_pcs(
    c_object: &Path,
    linked_pcs: &[u32],
    linked_cmain_addr: u32,
) -> Result<HashMap<u32, SourceLocation>, String> {
    if linked_pcs.is_empty() {
        return Ok(HashMap::new());
    }
    let mut child = Command::new("llvm-symbolizer")
        .arg(format!("--obj={}", c_object.display()))
        .arg("--inlining=false")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|err| format!("failed to run llvm-symbolizer for C object: {err}"))?;
    {
        let stdin = child
            .stdin
            .as_mut()
            .ok_or("failed to open C object llvm-symbolizer stdin")?;
        for pc in linked_pcs {
            let object_pc = pc.wrapping_sub(linked_cmain_addr);
            writeln!(stdin, "0x{object_pc:08x}").map_err(|err| err.to_string())?;
        }
    }
    let output = child
        .wait_with_output()
        .map_err(|err| format!("failed to read C object llvm-symbolizer output: {err}"))?;
    if !output.status.success() {
        return Err(format!(
            "C object llvm-symbolizer failed with status {}",
            output.status
        ));
    }

    let text = String::from_utf8_lossy(&output.stdout);
    let mut lines = text.lines();
    let mut out = HashMap::new();
    for linked_pc in linked_pcs {
        let _function = lines.next();
        let location = lines.next().unwrap_or("??:0:0");
        let _blank = lines.next();
        out.insert(*linked_pc, parse_location("cmain".to_string(), location));
    }
    Ok(out)
}

fn parse_location(function: String, location: &str) -> SourceLocation {
    let (file, line, column) = split_location(location);
    let external = !(file.contains("/benchmarks/")
        || file.contains("/include/")
        || file.contains("/zkvm-compiler-optimizations/programs/"));
    SourceLocation {
        function,
        file: if external {
            "<external>".to_string()
        } else {
            file
        },
        line,
        column,
        external,
    }
}

fn split_location(location: &str) -> (String, u64, u64) {
    if location == "??:0:0" || location == "??" {
        return ("<unknown>".to_string(), 0, 0);
    }
    let mut parts = location.rsplitn(3, ':').collect::<Vec<_>>();
    parts.reverse();
    if parts.len() != 3 {
        return (location.to_string(), 0, 0);
    }
    (
        parts[0].to_string(),
        parts[1].parse().unwrap_or(0),
        parts[2].parse().unwrap_or(0),
    )
}

fn render_hotspots(args: &Args, summary: &SourcePagingSummary) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "# Source Page Hotspots\n\nbenchmark: `{}`\nprofile: `{}`\nsize: `{}`\nsegment_model: `{}`\n\n",
        args.benchmark, args.profile, args.size, summary.segment_model
    ));
    out.push_str(&format!(
        "- synthetic page-ins: `{}`\n- synthetic page-outs: `{}`\n- unique pages loaded: `{}`\n- unique pages dirtied: `{}`\n- session paging cycles: `{}`\n\n",
        summary.total_synthetic_page_ins,
        summary.total_synthetic_page_outs,
        summary.unique_pages_loaded,
        summary.unique_pages_dirtied,
        summary.session_paging_cycles
    ));
    out.push_str("| page events | page-ins | page-outs | code-ins | load-ins | store-ins | dirty-outs | instructions | source |\n");
    out.push_str("|---:|---:|---:|---:|---:|---:|---:|---:|---|\n");
    for item in &summary.top_hotspots {
        let events = item.synthetic_page_ins + item.synthetic_page_outs;
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} | {} | {}:{} `{}` |\n",
            events,
            item.synthetic_page_ins,
            item.synthetic_page_outs,
            item.code_page_ins,
            item.load_page_ins,
            item.store_page_ins,
            item.store_page_outs,
            item.executed_instructions,
            item.file,
            item.line,
            item.function
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_load_with_signed_immediate() {
        let mut regs = [0u32; 32];
        regs[2] = 0x1000;
        let insn = (0xffcu32 << 20) | (2 << 15) | (2 << 12) | (1 << 7) | 0x03;
        assert_eq!(decode_load_addr(insn, &regs), Some(0x0ffc));
    }

    #[test]
    fn ignores_non_load_instruction() {
        let regs = [0u32; 32];
        assert_eq!(decode_load_addr(0x00000033, &regs), None);
    }

    #[test]
    fn tracks_first_touch_and_first_dirty() {
        let mut paging = SourcePaging::default();
        paging.touch_code(0x1000);
        paging.touch_code(0x1004);
        paging.touch_store(0x2000, 0x4000);
        paging.touch_store(0x2004, 0x4004);
        assert_eq!(paging.page_ins.values().sum::<u64>(), 2);
        assert_eq!(paging.page_outs.values().sum::<u64>(), 1);
    }
}
