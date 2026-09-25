//! 规则版判断后端：零依赖、< 1 ms、默认开启。
//!
//! 它不是「模型不够用时的临时替代」—— 按开发文档 §5.7 的三级闸门，
//! 它本来就是**第一级**。实测数据也支持这个设计：模型自己对纯关键词给出的
//! `is_natural` 只有 0.09~0.15，与自然语言的 0.92+ 分离度良好，
//! 说明本地规则与模型判断是一致的。
//!
//! 输出与模型后端**同构**（都是 `Answers`），所以换后端不需要改调用方。

use super::slots::{LocationSlot, TimeSlot, TypeSlot};
use super::{classify_input, contains_word, Answer, Answers, DecisionBackend, PASS_THRESHOLD};
use anyhow::Result;
use std::collections::HashMap;

/// 规则命中时的置信度。
///
/// 刻意不设成 1.0：规则是「词表命中」，不是语义理解。
/// 留在 0.85 这个档位，将来接本地模型时「低置信升级」才有空间。
const CONF_HIT: f32 = 0.85;

// ---------------------------------------------------------------------------
// 词表
// ---------------------------------------------------------------------------

/// ⚠️ **顺序即优先级**：先匹配到的胜出。
///
/// `Folder` 必须排在 `Document` 前面 —— 否则「我的项目**文件**夹在哪」
/// 会先被 `Document` 的「文件」命中，判成文档。
const TYPE_RULES: &[(TypeSlot, &[&str])] = &[
    (TypeSlot::Folder, &["文件夹", "目录", "folder", "directory"]),
    (TypeSlot::Archive, &["压缩包", "压缩文件", "archive", "zip", "rar", "7z", "tar"]),
    (
        TypeSlot::Image,
        &[
            "图片", "照片", "截图", "壁纸", "image", "photo", "screenshot", "picture", "png",
            "jpg", "jpeg", "gif", "svg", "bmp", "webp", "ico",
        ],
    ),
    (
        TypeSlot::Media,
        &[
            "视频", "音频", "音乐", "video", "audio", "movie", "music", "mp3", "mp4", "wav",
            "flac", "mkv", "avi", "mov",
        ],
    ),
    (
        TypeSlot::Executable,
        &["可执行", "安装包", "executable", "installer", "exe", "msi", "setup"],
    ),
    (
        TypeSlot::Code,
        &[
            "代码", "源码", "脚本", "配置", "code", "source", "script", "config", "rs", "py",
            "js", "ts", "tsx", "json", "yml", "yaml", "toml", "html", "css", "go", "java",
            "cpp", "cs", "sql", "sh", "bat",
        ],
    ),
    (
        TypeSlot::Document,
        &[
            "文档", "笔记", "说明", "document", "readme", "markdown", "pdf", "word", "txt",
            "ppt", "excel", "xls", "note",
        ],
    ),
];

/// 同样按优先级排列
const TIME_RULES: &[(TimeSlot, &[&str])] = &[
    (TimeSlot::Yesterday, &["昨天", "昨日", "yesterday"]),
    (TimeSlot::Today, &["今天", "今日", "刚刚", "刚才", "today"]),
    (TimeSlot::LastWeek, &["上周", "上星期", "last week"]),
    (TimeSlot::ThisWeek, &["本周", "这周", "这星期", "这几天", "this week"]),
    (TimeSlot::ThisMonth, &["本月", "这个月", "this month"]),
    (TimeSlot::ThisYear, &["今年", "this year"]),
    (TimeSlot::Older, &["更早", "以前", "很久", "older"]),
];

/// 位置：只用**明确**的位置词，避免与类型词打架（「文档」是类型，「文档目录」才是位置）
const LOCATION_COMMON: &[&str] = &["桌面", "下载", "desktop", "download"];
const LOCATION_CURRENT: &[&str] = &["当前目录", "这个文件夹", "这个目录", "这里", "current folder"];

/// 明显的**非检索**输入（闲聊 / 生成任务）
const NEGATIVE: &[&str] = &[
    "帮我写", "写一段", "写个", "实现一个", "解释一下", "什么是", "天气", "翻译",
    "write me", "explain", "what is the weather",
];

// ---------------------------------------------------------------------------
// 后端
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct RuleBackend;

impl RuleBackend {
    pub fn new() -> Self {
        RuleBackend
    }
}

impl DecisionBackend for RuleBackend {
    fn id(&self) -> &'static str {
        "rule"
    }

    fn display_name(&self) -> &'static str {
        "本地规则"
    }

    fn is_local(&self) -> bool {
        true
    }

    fn is_available(&self) -> bool {
        true
    }

    fn warm(&self) -> Result<()> {
        Ok(())
    }

    fn decide(&self, state: &str, _questions: &[super::Question]) -> Result<Answers> {
        Ok(solve(state))
    }
}

/// 纯函数版本，方便单测直接调用
pub fn solve(query: &str) -> Answers {
    let lower = query.trim().to_lowercase();
    let shape = classify_input(query);
    let mut out: Answers = HashMap::new();

    // ── 三个槽位 ──────────────────────────────────────────
    //
    // ⚠️ **未命中的槽位不放进 answers** —— 交给 `Intent::from_answers` 的回落
    // 机制去补 `all` / `any`。若在这里塞一个「all + 0.30」，
    // 会让「什么都没解析出来」也带着一个不低的置信度，把升级判定带偏。
    let mut put = |name: &str, slot: &str, conf: f32| {
        out.insert(
            name.into(),
            Answer::Choice {
                choice: slot.to_string(),
                confidence: conf,
                probabilities: HashMap::new(),
            },
        );
    };

    if let Some((slot, conf)) = first_hit(&lower, TYPE_RULES) {
        put("type", slot.as_str(), conf);
    }
    if let Some((slot, conf)) = first_hit(&lower, TIME_RULES) {
        put("time", slot.as_str(), conf);
    }
    if let Some((slot, conf)) = solve_location(query, &lower) {
        put("location", slot.as_str(), conf);
    }
    drop(put);

    // ── 是否在找东西 ──────────────────────────────────────
    // 规则版偏保守：只有明确命中闲聊/生成词表才判「不是搜索」，
    // 空输入也算不是（避免空查询触发一次无意义的模型调用）。
    let is_search = !lower.is_empty() && !NEGATIVE.iter().any(|w| lower.contains(w));
    out.insert(
        "is_search".into(),
        Answer::Noul { noul: if is_search { 0.90 } else { 0.10 } },
    );

    // ── 是否自然语言 ──────────────────────────────────────
    out.insert(
        "is_natural".into(),
        Answer::Noul {
            noul: if shape.is_natural { 0.90 } else { 0.10 },
        },
    );

    out
}

fn first_hit<T: Copy>(lower: &str, rules: &[(T, &[&str])]) -> Option<(T, f32)> {
    for (slot, words) in rules {
        // 必须走词边界匹配：词表里的 `rs` / `go` 这类短词
        // 用子串匹配会命中 `hours` / `goes`，类型判定直接失准。
        if words.iter().any(|w| contains_word(lower, w)) {
            return Some((*slot, CONF_HIT));
        }
    }
    None
}

/// 位置判定：先认盘符（最明确），再认当前目录，最后认常用目录。
/// 都没命中返回 `None` —— 交给调用方回落到「不限」。
fn solve_location(raw: &str, lower: &str) -> Option<(LocationSlot, f32)> {
    // 盘符：`D盘` / `d 盘` / `C:\` / `E:/`
    if has_drive_letter(raw) {
        return Some((LocationSlot::Drive, CONF_HIT));
    }
    if LOCATION_CURRENT.iter().any(|w| contains_word(lower, w)) {
        return Some((LocationSlot::Current, CONF_HIT));
    }
    if LOCATION_COMMON.iter().any(|w| contains_word(lower, w)) {
        return Some((LocationSlot::Common, CONF_HIT));
    }
    None
}

/// 识别 `D盘` / `d盘` / `C:\` / `E:/` 这类写法
fn has_drive_letter(raw: &str) -> bool {
    let chars: Vec<char> = raw.chars().collect();
    for i in 0..chars.len() {
        let c = chars[i];
        if !c.is_ascii_alphabetic() {
            continue;
        }
        // 必须处在词首（前一个字符不是字母/数字），避免把 `docker盘` 误判成盘符
        if i > 0 && (chars[i - 1].is_ascii_alphanumeric()) {
            continue;
        }
        let next = chars.get(i + 1).copied();
        match next {
            Some('盘') => return true,
            Some(':') => {
                // 后一个可以是 \ 或 /，也可以直接结束
                if matches!(chars.get(i + 2), None | Some('\\') | Some('/')) {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

/// 把 noul 概率按阈值转成 bool（与探针同一判据）
pub fn noul_is_true(v: f32) -> bool {
    v >= PASS_THRESHOLD
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::decision::{standard_questions, Intent};

    fn intent(q: &str) -> Intent {
        let b = RuleBackend::new();
        let a = b.decide(q, &standard_questions()).unwrap();
        Intent::from_answers(&a, b.id())
    }

    #[test]
    fn type_slots_on_real_queries() {
        assert_eq!(intent("找一下昨天改的 docker 配置").type_slot, TypeSlot::Code);
        assert_eq!(intent("那个写存储逻辑的文档").type_slot, TypeSlot::Document);
        assert_eq!(intent("上周下载的那个压缩包").type_slot, TypeSlot::Archive);
        assert_eq!(intent("any png screenshots from last week").type_slot, TypeSlot::Image);
        assert_eq!(intent("随便来点东西").type_slot, TypeSlot::All);
    }

    #[test]
    fn folder_beats_document() {
        // 词表顺序的回归测试：「文件夹」含「文件」，
        // 若 Document 排在前面就会误判成文档
        assert_eq!(intent("我的项目文件夹在哪").type_slot, TypeSlot::Folder);
    }

    #[test]
    fn time_slots_on_real_queries() {
        assert_eq!(intent("找一下昨天改的 docker 配置").time_slot, TimeSlot::Yesterday);
        assert_eq!(intent("上周下载的那个压缩包").time_slot, TimeSlot::LastWeek);
        assert_eq!(intent("files I modified yesterday").time_slot, TimeSlot::Yesterday);
        assert_eq!(intent("any png screenshots from last week").time_slot, TimeSlot::LastWeek);
        assert_eq!(intent("docker").time_slot, TimeSlot::Any);
    }

    #[test]
    fn location_drive_letters() {
        assert_eq!(intent("D盘的项目").location_slot, LocationSlot::Drive);
        assert_eq!(intent("C:\\Users 里的东西").location_slot, LocationSlot::Drive);
        assert_eq!(intent("桌面上的截图").location_slot, LocationSlot::Common);
        assert_eq!(intent("当前目录的文档").location_slot, LocationSlot::Current);
        assert_eq!(intent("docker").location_slot, LocationSlot::Any);
        // `docker盘` 不是盘符（前面还有字母）
        assert_ne!(
            solve_location("docker盘", "docker盘").map(|(s, _)| s),
            Some(LocationSlot::Drive)
        );
    }

    #[test]
    fn non_search_inputs_are_rejected() {
        assert!(!intent("今天天气怎么样").is_search);
        assert!(!intent("帮我写一段快排").is_search);
        assert!(!intent("").is_search);
        // 真正的搜索必须通过
        for q in ["找一下昨天改的 docker 配置", "docker", "storage.rs", "我的项目文件夹在哪"] {
            assert!(intent(q).is_search, "{q} 应该被判为搜索");
        }
    }

    #[test]
    fn natural_language_flag_matches_gate1() {
        assert!(!intent("docker").is_natural);
        assert!(!intent("storage.rs").is_natural);
        assert!(intent("找一下昨天改的 docker 配置").is_natural);
        assert!(intent("the doc about storage logic").is_natural);
    }

    #[test]
    fn unmatched_slots_are_omitted_not_defaulted() {
        // 三个槽位一个都没命中时，不该往 answers 里塞 all/any 占位 ——
        // 否则「什么都没解析出来」会带着一个不低的置信度
        let a = solve("docker");
        assert!(!a.contains_key("type"), "未命中的槽位不该塞进 answers");
        assert!(!a.contains_key("time"));
        assert!(!a.contains_key("location"));
        // 但两个二值问题一定答
        assert!(a.contains_key("is_search"));
        assert!(a.contains_key("is_natural"));

        // 命中的槽位必须在，没提到的仍然不该有
        let a = solve("找一下昨天改的 docker 配置");
        assert!(a.contains_key("type"));
        assert!(a.contains_key("time"));
        assert!(!a.contains_key("location"), "没提到位置就不该有 location");
    }

    #[test]
    fn rule_backend_is_always_available() {
        let b = RuleBackend::new();
        assert!(b.is_available());
        assert!(b.is_local());
        assert_eq!(b.id(), "rule");
        assert!(b.warm().is_ok());
    }
}
