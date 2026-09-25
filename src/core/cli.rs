//! 索引维护命令行入口。
//!
//! 存在的理由有两个：
//! 1. **能在数据库副本上验证**。`--db` 可以指向任意路径，
//!    于是可以在不动真实库的前提下跑迁移、对账、空间回收。
//! 2. 不启动 GUI 也能查看与维护索引，出问题时排查成本低得多。
//!
//! ```text
//! anycast --index-status  [--db PATH]                     # 一致性 + 空间占用
//! anycast --index-compact [--db PATH]                     # WAL checkpoint + VACUUM
//! anycast --index-scan    [--db PATH] [--root DIR]... [--no-content] [--full|--rebuild]
//! anycast --index-watch   [--db PATH] --root DIR [--seconds N]   # 挂实时监控 N 秒
//! ```
//!
//! `--index-scan` 会对给定目录跑**一轮真实对账**并打印统计，
//! 是验证增量逻辑最直接的方式。默认是增量对账；
//! `--full` 强制检查每个目录的子项（不清空），`--rebuild` 则先清空再全量。
//!
//! `--index-watch` 走的是**真实的 notify 实时路径**（`Indexer::start()`），
//! 用来验证「改名事件是否稳定带新旧两个路径」这类只在真机上才看得出的问题。

use crate::core::indexer::Indexer;
use crate::core::settings::AppSettings;
use crate::core::storage::Storage;
use std::path::PathBuf;
use std::sync::Arc;

pub const USAGE: &str = "\
索引维护命令：
  --index-status  [--db PATH]                        一致性快照与空间占用
  --index-compact [--db PATH]                        回收空间（WAL checkpoint + VACUUM）
  --index-scan    [--db PATH] [--root DIR]... [--no-content] [--full|--rebuild]
                                                     对指定目录跑一轮真实索引
  --index-watch   [--db PATH] --root DIR [--seconds N]
                                                     挂实时监控 N 秒（默认 20），
                                                     用于验证 notify 事件形态
  --decide QUERY  [--cloud]                          解析查询的意图槽位：闸门 1 判定 +
                                                     规则版槽位 + 埋点报告。
                                                     --cloud 额外试一次云端 Jev（需 Key）
  --db PATH       指定数据库（默认 %APPDATA%\\Anycast\\data\\anycast.db）
  --root DIR      索引根目录（可重复）
  --seconds N     监控时长

扫描模式：默认增量对账（只处理 mtime 变化的目录）
  --full      强制检查每个目录的子项（不清空），用于补回
              应用未运行期间被改写内容、目录 mtime 却不变化的文件
  --rebuild   先清空再全量重建

注意：release 构建带 windows_subsystem=\"windows\"，不挂控制台，
      输出需重定向到文件：anycast.exe --index-status > status.txt
";

struct Args {
    status: bool,
    compact: bool,
    scan: bool,
    watch: bool,
    rebuild: bool,
    full: bool,
    no_content: bool,
    seconds: u64,
    db: Option<PathBuf>,
    roots: Vec<String>,
    /// `--decide QUERY`
    decide: Option<String>,
    /// `--cloud`：--decide 时额外试一次云端
    cloud: bool,
}

fn parse(argv: &[String]) -> Option<Args> {
    if !argv
        .iter()
        .any(|a| a.starts_with("--index-") || a == "--decide")
    {
        return None;
    }
    let mut a = Args {
        status: false,
        compact: false,
        scan: false,
        watch: false,
        rebuild: false,
        full: false,
        no_content: false,
        seconds: 20,
        db: None,
        roots: Vec::new(),
        decide: None,
        cloud: false,
    };
    let mut it = argv.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--index-status" => a.status = true,
            "--index-compact" => a.compact = true,
            "--index-scan" => a.scan = true,
            "--index-watch" => a.watch = true,
            "--rebuild" => a.rebuild = true,
            "--full" => a.full = true,
            "--no-content" => a.no_content = true,
            "--seconds" => a.seconds = it.next().and_then(|s| s.parse().ok()).unwrap_or(20),
            "--db" => a.db = it.next().map(PathBuf::from),
            "--decide" => a.decide = it.next().cloned(),
            "--cloud" => a.cloud = true,
            "--root" => {
                if let Some(r) = it.next() {
                    a.roots.push(r.clone());
                }
            }
            "--help" | "-h" => {
                println!("{USAGE}");
                std::process::exit(0);
            }
            _ => {}
        }
    }
    Some(a)
}

fn mb(b: i64) -> String {
    format!("{:.2} MB", b as f64 / 1_048_576.0)
}

fn print_integrity(s: &Storage, title: &str) {
    let r = s.integrity();
    println!("\n=== {title} ===");
    println!("  文件索引      {} 条（其中目录 {} 条）", r.files, r.dirs);
    println!("  正文索引      {} 条（元数据 {} 条）", r.content, r.content_meta);
    println!(
        "  孤儿正文      {} 条{}",
        r.orphan_content,
        if r.orphan_content > 0 { "   ← 搜得到但打不开" } else { "" }
    );
    println!("  悬空置顶      {} 条", r.dangling_pins);
    println!("  悬空最近使用  {} 条", r.dangling_recent);
    println!("  数据库大小    {}", mb(r.db_bytes));
    println!(
        "  空闲页        {}  ({:.1}%)",
        mb(r.freelist_bytes),
        if r.db_bytes > 0 { r.freelist_bytes as f64 * 100.0 / r.db_bytes as f64 } else { 0.0 }
    );
    println!("  一致性        {}", if r.healthy() { "正常" } else { "存在不一致" });
}

/// 命中命令行模式时返回退出码；否则返回 None，交回 GUI 启动流程。
pub fn maybe_run(argv: &[String]) -> Option<i32> {
    let a = parse(argv)?;
    if !a.status && !a.compact && !a.scan && !a.watch && a.decide.is_none() {
        println!("{USAGE}");
        return Some(2);
    }
    match run(a) {
        Ok(code) => Some(code),
        Err(e) => {
            eprintln!("执行失败: {e:#}");
            Some(1)
        }
    }
}

fn run(a: Args) -> anyhow::Result<i32> {
    let db = a.db.clone().unwrap_or_else(crate::core::settings::db_path);
    println!("数据库: {}", db.display());
    let storage = Arc::new(Storage::open(&db)?);

    if let Some(q) = a.decide.clone() {
        run_decide(&storage, &q, a.cloud);
    }

    if a.status {
        print_integrity(&storage, "索引一致性");
    }

    if a.compact {
        print_integrity(&storage, "整理前");
        let started = std::time::Instant::now();
        let r = storage.maintenance(true)?;
        println!(
            "\n整理完成（耗时 {:?}）：{} → {}，空闲页 {} → {}{}",
            started.elapsed(),
            mb(r.before_bytes),
            mb(r.after_bytes),
            mb(r.before_free),
            mb(r.after_free),
            if r.vacuumed { "" } else { "（未触发 VACUUM）" }
        );
        print_integrity(&storage, "整理后");
    }

    if a.scan {
        if a.roots.is_empty() {
            anyhow::bail!("--index-scan 需要至少一个 --root 目录");
        }
        let settings = AppSettings {
            index_roots: a.roots.clone(),
            content_index_enabled: !a.no_content,
            ..Default::default()
        };
        let mode = if a.rebuild { 2 } else if a.full { 1 } else { 0 };
        println!("\n=== 索引扫描 ===");
        println!("  根目录: {:?}", settings.index_roots);
        println!(
            "  模式:   {}，正文索引 {}",
            match mode {
                2 => "清空重建",
                1 => "全量校验（不清空）",
                _ => "增量对账",
            },
            if settings.content_index_enabled { "开" } else { "关" }
        );

        let indexer = Indexer::new(Arc::clone(&storage), settings, Arc::new(|_| {}));
        let started = std::time::Instant::now();
        indexer.scan_blocking(mode);
        let st = indexer.status();
        println!("\n  耗时            {:?}", started.elapsed());
        println!("  模式            {}", st.mode);
        println!("  目录访问/变化   {} / {}", st.dirs_visited, st.dirs_changed);
        println!("  索引文件        {} 条", st.files);
        println!("  正文索引        {} 条", st.content_files);
        println!("  移除条目        {} 条", st.removed);
        println!("  丢弃条目        {} 条{}", st.dropped,
            if st.dropped > 0 { "   ← 异常，需排查" } else { "" });
        if let Some(e) = &st.error {
            println!("  最近错误        {e}");
        }
        print_integrity(&storage, "扫描后");
    }

    if a.watch {
        if a.roots.is_empty() {
            anyhow::bail!("--index-watch 需要至少一个 --root 目录");
        }
        let settings = AppSettings {
            index_roots: a.roots.clone(),
            content_index_enabled: !a.no_content,
            ..Default::default()
        };
        println!("\n=== 实时监控 ===");
        println!("  根目录: {:?}", settings.index_roots);
        println!("  时长:   {} 秒", a.seconds);
        println!("  （走真实 notify 路径，期间对目录做增删改名即可观察效果）");

        let indexer = Indexer::new(Arc::clone(&storage), settings, Arc::new(|_| {}));
        indexer.start();
        // 等首轮对账落定（最多 30 秒）再开始计时
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while indexer.status().last_scan_ms == 0 && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        println!("  首轮对账完成，开始计时");
        std::thread::sleep(std::time::Duration::from_secs(a.seconds));

        let st = indexer.status();
        println!("\n  首轮耗时        {} ms", st.last_scan_ms);
        println!("  目录访问/变化   {} / {}", st.dirs_visited, st.dirs_changed);
        println!("  索引文件        {} 条", st.files);
        println!("  移除条目        {} 条", st.removed);
        println!("  丢弃条目        {} 条", st.dropped);
        if let Some(e) = &st.error {
            println!("  最近错误        {e}");
        }
        print_integrity(&storage, "监控结束后");
    }

    Ok(0)
}

// ---------------------------------------------------------------------------
// 判断模型验证（--decide）
// ---------------------------------------------------------------------------

fn yn(v: bool) -> &'static str {
    if v {
        "是"
    } else {
        "否"
    }
}

fn fmt_ts(ts: i64) -> String {
    use chrono::{Local, TimeZone};
    Local
        .timestamp_opt(ts, 0)
        .single()
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(|| ts.to_string())
}

fn indent(text: &str) -> String {
    text.lines()
        .map(|l| format!("    {l}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn print_intent(it: &crate::core::decision::Intent) {
    println!("    类型          {}（{}）", it.type_slot.label_cn(), it.type_slot.as_str());
    println!("    时间          {}（{}）", it.time_slot.label_cn(), it.time_slot.as_str());
    println!("    位置          {}（{}）", it.location_slot.label_cn(), it.location_slot.as_str());
    println!("    是搜索        {}", yn(it.is_search));
    println!("    是自然语言    {}", yn(it.is_natural));
    println!("    置信度        {:.2}", it.confidence);
}

/// 打印一次查询的完整解析链路。
///
/// 存在理由：GUI 里只能看到「结果变了」，看不到**为什么**变 ——
/// 闸门判定、槽位取值、置信度、埋点，这些在命令行里才看得清。
fn run_decide(storage: &Arc<Storage>, query: &str, cloud: bool) {
    use crate::core::decision::hub::DecisionHub;
    use crate::core::decision::jev;

    let mut settings = crate::core::settings::load_settings();
    // 诊断工具：不受「功能未启用」影响 —— 否则开关一关就什么都看不到。
    // 只影响本次进程，不写回配置。
    if !settings.ai_enabled || !settings.ai_intent_parsing {
        println!("\n注：配置里 AI 功能未启用，本次按诊断模式强制打开（不会改动你的配置）");
        settings.ai_enabled = true;
        settings.ai_intent_parsing = true;
    }
    let hub = DecisionHub::new(Arc::clone(storage));

    let t0 = std::time::Instant::now();
    let a = hub.analyze(query, &settings);
    let elapsed = t0.elapsed();

    println!("\n=== 判断模型 · 闸门 1 ===");
    println!("  查询            {query}");
    println!("  耗时            {elapsed:?}");
    println!("  语言            {}", a.shape.lang.as_str());
    println!("  估算词数        {}", a.shape.words);
    println!("  像自然语言      {}", yn(a.shape.is_natural));
    println!("  像文件名        {}", yn(a.shape.filename_like));
    println!("  含时间词        {}", yn(a.shape.has_time_word));
    println!("  含类型词        {}", yn(a.shape.has_type_word));
    println!("  值得调模型      {}", yn(a.worth_upgrade));
    println!("  预计闸门        {}", a.gate.as_str());
    println!("  规则解析出槽位  {}", yn(a.slot_hit));

    println!("\n  规则版槽位解析");
    print_intent(&a.intent);
    let (from, to) = a.intent.time_slot.range();
    if from.is_some() || to.is_some() {
        println!(
            "    时间范围      {} → {}",
            from.map(fmt_ts).unwrap_or_else(|| "不限".into()),
            to.map(fmt_ts).unwrap_or_else(|| "不限".into())
        );
    }

    if cloud {
        println!("\n  云端 Jev（闸门 3）");
        if !hub.cloud_ready(&settings) {
            println!("    跳过：未启用云端，或未配置 API Key");
            println!("    提示：设置环境变量 {} 或 {}，",
                jev::ENV_OFFICIAL, jev::ENV_HOSTED);
            println!("          或把 key 写进 config.json 的 ai_jev_api_key");
        } else {
            let t1 = std::time::Instant::now();
            match hub.upgrade(query, &settings) {
                Ok(Some(it)) => {
                    println!("    耗时          {:?}", t1.elapsed());
                    print_intent(&it);
                }
                Ok(None) => println!(
                    "    跳过：闸门 1 判定无需升级（纯关键词，或规则已解析出槽位）"
                ),
                Err(e) => println!("    失败（已静默降级到规则结果）：{e}"),
            }
        }
    }

    println!("\n  埋点累计");
    println!("{}", indent(&hub.telemetry_report()));
    hub.flush_telemetry();
}
