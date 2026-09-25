//! 判断模型接入 —— System One 式决策后端。
//!
//! # 定位（开发文档 §5.1）
//!
//! 只做两件事：**意图解析**（槽位）+ **查询路由**。
//!
//! **明确不做**：
//! - ❌ 不生成关键词（关键词必须由本地分词产出，模型只输出枚举）
//! - ❌ 不生成文本、不回答开放问题
//! - ❌ **不做结果重排打分** —— 官方 `search relevance` 仅 0.628、
//!   `response quality scoring` 仅 0.581，而 `intent and routing` 有 0.991。
//!   重排交给本地排序算法（`search.rs` 的 `name_score` + `boost_by_usage`）。
//! - ❌ 不做算术 / 计数 / 日期比较（用确定性代码）
//!
//! # 三级闸门（§5.7）
//!
//! ```text
//! 输入
//!  ├─ 闸门 1 · 本地规则（<1ms）   纯关键词 → 直接检索，【不调模型】
//!  ├─ 闸门 2 · 本地模型（200~460ms）  先出本地结果 → 异步解析 → 二次检索
//!  └─ 闸门 3 · 云端 Jev（p50 ≈ 900ms）  仅显式开启且低置信时；硬超时 2000ms
//! ```
//!
//! 三条硬性规则：**首屏永远由本地检索提供**、**失败静默降级**、
//! **纯关键词永不触发闸门 2/3**。

pub mod bridge;
pub mod hub;
pub mod jev;
pub mod rule;
pub mod slots;
pub mod telemetry;

use anyhow::Result;
use std::collections::HashMap;

use slots::{LocationSlot, TimeSlot, TypeSlot};

// ---------------------------------------------------------------------------
// 通用类型
// ---------------------------------------------------------------------------

/// 问题类型。三种的 `criteria` 写法不同，容易踩坑（§2.1.1）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuestionKind {
    /// `criteria` 是**映射**（key → 描述），最多 255 个选项
    Choice,
    /// `criteria` 是**有序数组**（2~10 个等级描述），返回可能带小数
    Score,
    /// **不需要** `criteria`，仅 `instructions`，返回 0~1 概率
    Noul,
}

#[derive(Debug, Clone)]
pub enum Criteria {
    /// choice：选项 key → 描述
    Map(Vec<(&'static str, &'static str)>),
    /// score：有序等级描述
    List(Vec<&'static str>),
    /// noul
    None,
}

#[derive(Debug, Clone)]
pub struct Question {
    pub name: &'static str,
    pub kind: QuestionKind,
    pub instructions: &'static str,
    pub criteria: Criteria,
}

/// 一个槽位的答案
#[derive(Debug, Clone)]
pub enum Answer {
    Choice { choice: String, confidence: f32, probabilities: HashMap<String, f32> },
    Score { score: f32, confidence: f32 },
    Noul { noul: f32 },
}

impl Answer {
    /// 归一化成「一个字符串 + 一个置信度」，便于统一处理
    pub fn as_choice(&self) -> Option<(&str, f32)> {
        match self {
            Answer::Choice { choice, confidence, .. } => Some((choice.as_str(), *confidence)),
            _ => None,
        }
    }

    /// noul 概率；非 noul 返回 None
    pub fn as_noul(&self) -> Option<f32> {
        match self {
            Answer::Noul { noul } => Some(*noul),
            _ => None,
        }
    }

    pub fn confidence(&self) -> f32 {
        match self {
            Answer::Choice { confidence, .. } | Answer::Score { confidence, .. } => *confidence,
            Answer::Noul { noul } => *noul,
        }
    }
}

pub type Answers = HashMap<String, Answer>;

/// noul 概率的判定阈值（与探针一致）
pub const PASS_THRESHOLD: f32 = 0.5;

/// 标准问题集：三个槽位 + 两个二值问题。
///
/// **一次调用全部发出** —— 官方明确建议批量（并行执行、共享 state 成本），
/// 拆成多次既慢又贵。
pub fn standard_questions() -> Vec<Question> {
    vec![
        Question {
            name: "type",
            kind: QuestionKind::Choice,
            instructions: slots::INSTRUCTIONS_TYPE,
            criteria: Criteria::Map(slots::TYPE_OPTIONS.to_vec()),
        },
        Question {
            name: "time",
            kind: QuestionKind::Choice,
            instructions: slots::INSTRUCTIONS_TIME,
            criteria: Criteria::Map(slots::TIME_OPTIONS.to_vec()),
        },
        Question {
            name: "location",
            kind: QuestionKind::Choice,
            instructions: slots::INSTRUCTIONS_LOCATION,
            criteria: Criteria::Map(slots::LOCATION_OPTIONS.to_vec()),
        },
        Question {
            name: "is_search",
            kind: QuestionKind::Noul,
            instructions: slots::INSTRUCTIONS_IS_SEARCH,
            criteria: Criteria::None,
        },
        Question {
            name: "is_natural",
            kind: QuestionKind::Noul,
            instructions: slots::INSTRUCTIONS_IS_NATURAL,
            criteria: Criteria::None,
        },
    ]
}

// ---------------------------------------------------------------------------
// 后端抽象
// ---------------------------------------------------------------------------

pub trait DecisionBackend: Send + Sync {
    /// `"rule"` | `"laya"` | `"jev"`
    fn id(&self) -> &'static str;
    fn display_name(&self) -> &'static str;
    fn is_local(&self) -> bool;
    /// 权重存在 / API Key 已配置
    fn is_available(&self) -> bool;
    /// 预加载（仅本地模型需要）
    fn warm(&self) -> Result<()>;
    fn decide(&self, state: &str, questions: &[Question]) -> Result<Answers>;
}

/// 把原始 `Answers` 翻译成业务语义的结论
#[derive(Debug, Clone)]
pub struct Intent {
    pub is_search: bool,
    pub is_natural: bool,
    pub type_slot: TypeSlot,
    pub time_slot: TimeSlot,
    pub location_slot: LocationSlot,
    /// 位置的具体范围键，**只有规则后端能给**（云端只回枚举）。
    ///
    /// 取值与 UI 位置菜单 id 同构：`drive-c` / `drive-d` / `desktop` / `downloads`，
    /// 可直接塞进 `SearchScopeFilter::location_scope`。
    /// `LocationSlot::Current` 永远是 `None`（无头状态下无从得知「当前」是哪个目录）。
    pub location_scope: Option<String>,
    /// 各槽位置信度的最小值 —— 用于决定是否升级到下一级闸门
    pub confidence: f32,
    pub backend: &'static str,
}

impl Intent {
    /// 是否解析出了**至少一个**有效槽位。
    ///
    /// 这是「要不要花钱调云端」的实际判据 —— 规则已经认出来了就别再问模型。
    /// 也是「模型这次到底说了什么」的判据：全 `all` / `any` 等于什么都没说，
    /// 此时不该拿它去改检索范围。
    pub fn has_slot(&self) -> bool {
        self.type_slot != TypeSlot::All
            || self.time_slot != TimeSlot::Any
            || self.location_slot != LocationSlot::Any
    }

    /// 从任意后端返回的 `Answers` 组装结论。
    ///
    /// 缺失的槽位一律回落到最宽松的取值（`all` / `any`），
    /// **不因为某个槽位没答就整体失败** —— 静默降级是硬性要求。
    pub fn from_answers(answers: &Answers, backend: &'static str) -> Self {
        let noul = |name: &str, fallback: bool| -> bool {
            answers
                .get(name)
                .and_then(|a| a.as_noul())
                .map(|v| v >= PASS_THRESHOLD)
                .unwrap_or(fallback)
        };

        let choice = |name: &str| -> Option<(&str, f32)> {
            answers.get(name).and_then(|a| a.as_choice())
        };

        // 置信度只统计**真的解析出槽位**的那些问题。
        //
        // 把回落到 `all` / `any` 的槽位也算进来会得到一个误导性的数字：
        // 未命中时后端给的是低分（规则的 0.30），它会把最小值拖住，
        // 于是「什么都没解析出来」也显示成 0.30 而不是 0。
        let mut slot_confs: Vec<f32> = Vec::new();

        let type_slot = choice("type")
            .and_then(|(c, conf)| {
                slot_confs.push(conf);
                TypeSlot::parse(c)
            })
            .unwrap_or(TypeSlot::All);

        let time_slot = choice("time")
            .and_then(|(c, conf)| {
                slot_confs.push(conf);
                TimeSlot::parse(c)
            })
            .unwrap_or(TimeSlot::Any);

        let location_slot = choice("location")
            .and_then(|(c, conf)| {
                slot_confs.push(conf);
                LocationSlot::parse(c)
            })
            .unwrap_or(LocationSlot::Any);

        // 一个槽位都没解析出来 → 置信度 0
        let confidence = slot_confs.iter().copied().fold(f32::INFINITY, f32::min);
        let confidence = if confidence.is_finite() {
            confidence.clamp(0.0, 1.0)
        } else {
            0.0
        };

        Intent {
            // is_search 缺失时保守地认为「是搜索」—— 宁可多给结果，不要吞掉用户的意图
            is_search: noul("is_search", true),
            // is_natural 缺失时按「不是自然语言」处理 —— 这样闸门会倾向于保守
            is_natural: noul("is_natural", false),
            type_slot,
            time_slot,
            location_slot,
            // 具体范围键不进 `slot_confs` —— 它是 `location` 的**补充**，
            // 与 `location` 同一次判定，重复计入会把置信度算成「两票」。
            location_scope: choice("location_scope").map(|(c, _)| c.to_string()),
            confidence,
            backend,
        }
    }
}

// ---------------------------------------------------------------------------
// 闸门 1：本地规则判定输入形态（零成本、零延迟）
// ---------------------------------------------------------------------------

/// 查询的语言构成。**这是决定「装哪个 checkpoint」的唯一依据**（§5.8），
/// 不能靠「文件都是英文的」去推断「查询是英文的」。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    /// 以拉丁字母为主
    Latin,
    /// 以 CJK 为主
    Cjk,
    /// 中英混合
    Mixed,
    /// 数字/符号等
    Other,
}

impl Lang {
    pub fn as_str(self) -> &'static str {
        match self {
            Lang::Latin => "latin",
            Lang::Cjk => "cjk",
            Lang::Mixed => "mixed",
            Lang::Other => "other",
        }
    }
}

/// 闸门 1 的判定结果
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputShape {
    /// 像自然语言（含时间词 / 疑问词 / 动词 / 多个词）
    pub is_natural: bool,
    /// 像文件名（`storage.rs`、`docker-compose.yml`）
    pub filename_like: bool,
    /// 含时间词（昨天 / 上周 / …）
    pub has_time_word: bool,
    /// 含类型词（文档 / 图片 / 压缩包 / …）
    pub has_type_word: bool,
    pub lang: Lang,
    /// 词数（按空白与常见分隔切分）
    pub words: usize,
}

impl InputShape {
    /// **是否值得调模型**。这是闸门 1 的全部意义：省电、保速度。
    ///
    /// 判据（§5.2）：纯关键词永不触发。实测佐证 —— 模型自己对纯关键词
    /// 给出的 `is_natural` 只有 0.09~0.15，与自然语言的 0.92+ 分离度良好，
    /// 说明本地规则与模型判断是一致的。
    pub fn worth_model(&self) -> bool {
        if self.filename_like {
            return false;
        }
        // 单个孤立词（`docker`、`vs`）不调
        if self.words <= 1 && !self.has_time_word && !self.has_type_word {
            return false;
        }
        self.is_natural || self.has_time_word || self.has_type_word
    }
}

const TIME_WORDS: &[&str] = &[
    "今天", "今日", "刚刚", "刚才", "昨天", "昨日", "前天", "本周", "这周", "这星期", "上周",
    "上星期", "本月", "这个月", "上个月", "上月", "今年", "去年", "更早", "以前", "最近",
    "today", "yesterday", "tomorrow", "week", "month", "year", "recent", "latest", "last",
    "this",
];

const TYPE_WORDS: &[&str] = &[
    "文档", "文件", "笔记", "说明", "代码", "源码", "脚本", "配置", "图片", "照片", "截图",
    "视频", "音频", "音乐", "压缩包", "文件夹", "目录", "程序", "安装包", "exe", "doc",
    "document", "file", "note", "code", "script", "config", "image", "photo", "screenshot",
    "video", "audio", "archive", "zip", "folder", "directory", "png", "jpg",
];

const VERB_WORDS: &[&str] = &[
    "找", "找找", "找一下", "搜", "搜索", "查", "查找", "打开", "看看", "给我", "帮我", "在哪",
    "在哪里", "哪个", "什么", "怎么", "为什么", "是不是", "有没有",
    "find", "search", "open", "show", "where", "which", "what", "locate", "look",
];

/// 判定输入形态（闸门 1）
pub fn classify_input(q: &str) -> InputShape {
    let raw = q.trim();
    let lower = raw.to_lowercase();

    let (latin, cjk) = count_scripts(raw);
    let lang = if latin > 0 && cjk > 0 {
        // 少数几个拉丁字符（如 `docker 配置`）不算混合
        if latin >= 3 && cjk >= 1 {
            Lang::Mixed
        } else if cjk > 0 {
            Lang::Cjk
        } else {
            Lang::Latin
        }
    } else if cjk > 0 {
        Lang::Cjk
    } else if latin > 0 {
        Lang::Latin
    } else {
        Lang::Other
    };

    let has_time_word = TIME_WORDS.iter().any(|w| contains_word(&lower, w));
    let has_type_word = TYPE_WORDS.iter().any(|w| contains_word(&lower, w));
    let has_verb = VERB_WORDS.iter().any(|w| contains_word(&lower, w));

    let words = estimate_words(raw);

    // 像文件名：含扩展名且没有空格（`storage.rs`、`docker-compose.yml`）
    let filename_like = !raw.contains(char::is_whitespace) && looks_like_filename(&lower);

    // 像自然语言：含动词 / 时间词 / 疑问号 / 词数 ≥ 3
    let is_natural = has_verb
        || has_time_word
        || raw.contains('?')
        || raw.contains('？')
        || (words >= 3 && lang != Lang::Other);

    InputShape { is_natural, filename_like, has_time_word, has_type_word, lang, words }
}

fn is_cjk(c: char) -> bool {
    matches!(c as u32, 0x4E00..=0x9FFF | 0x3400..=0x4DBF | 0xF900..=0xFAFF)
}

fn count_scripts(s: &str) -> (usize, usize) {
    let mut latin = 0;
    let mut cjk = 0;
    for c in s.chars() {
        if c.is_ascii_alphabetic() {
            latin += 1;
        } else if is_cjk(c) {
            cjk += 1;
        }
    }
    (latin, cjk)
}

/// 词表匹配。**英文词必须按词边界匹配**，中文词直接子串匹配。
///
/// ⚠️ 不加词边界的后果是实打实的 bug：`doc` 会命中 `docker`，
/// 于是 `docker` 被当成「含类型词」，闸门 1 直接失效、纯关键词被送去调模型。
/// 允许单个 `s` 后缀是为了覆盖 `files` / `docs` 这类复数。
fn contains_word(haystack: &str, needle: &str) -> bool {
    if !needle.is_ascii() {
        return haystack.contains(needle);
    }
    let mut from = 0;
    while let Some(i) = haystack[from..].find(needle) {
        let start = from + i;
        let end = start + needle.len();
        let before_ok = start == 0
            || !haystack[..start]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_ascii_alphanumeric());
        let rest = &haystack[end..];
        let after_ok = match rest.chars().next() {
            None => true,
            Some(c) if !c.is_ascii_alphanumeric() => true,
            // 复数后缀：`files` 里的 `file`
            Some('s') => rest[1..].chars().next().map_or(true, |c| !c.is_ascii_alphanumeric()),
            _ => false,
        };
        if before_ok && after_ok {
            return true;
        }
        from = end;
    }
    false
}

/// 估算词数。
///
/// **中文没有空格**，`split_whitespace` 对「我前几天弄的那个东西」恒为 1 ——
/// 照它判就会把整句中文当成单个孤立词，永远判不出自然语言。
/// 中文按平均词长 2 字折算。
fn estimate_words(raw: &str) -> usize {
    let ascii_words = raw.split_whitespace().count();
    let cjk_words = raw.chars().filter(|c| is_cjk(*c)).count() / 2;
    ascii_words.max(cjk_words).max(if raw.is_empty() { 0 } else { 1 })
}

/// `xxx.ext` 形式，且扩展名是常见的 1~8 位字母数字
fn looks_like_filename(lower: &str) -> bool {
    let Some((stem, ext)) = lower.rsplit_once('.') else {
        return false;
    };
    !stem.is_empty()
        && !ext.is_empty()
        && ext.len() <= 8
        && ext.chars().all(|c| c.is_ascii_alphanumeric())
        && stem.chars().all(|c| !c.is_whitespace())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pure_keywords_never_call_model() {
        // 闸门 1 的核心承诺：这些必须一个都不调模型
        for q in ["docker", "vs", "storage.rs", "docker-compose.yml", "main.rs"] {
            let s = classify_input(q);
            assert!(!s.worth_model(), "{q} 不该触发模型：{s:?}");
        }
    }

    #[test]
    fn natural_language_does_call_model() {
        for q in [
            "找一下昨天改的 docker 配置",
            "那个写存储逻辑的文档",
            "我的项目文件夹在哪",
            "上周下载的那个压缩包",
            "files I modified yesterday",
            "the doc about storage logic",
        ] {
            let s = classify_input(q);
            assert!(s.worth_model(), "{q} 应该触发模型：{s:?}");
        }
    }

    #[test]
    fn language_classification() {
        assert_eq!(classify_input("docker").lang, Lang::Latin);
        assert_eq!(classify_input("昨天改的文档").lang, Lang::Cjk);
        assert_eq!(classify_input("找 Dockerfile 昨天改的").lang, Lang::Mixed);
        assert_eq!(classify_input("storage.rs").lang, Lang::Latin);
    }

    #[test]
    fn filename_like_detection() {
        assert!(classify_input("storage.rs").filename_like);
        assert!(classify_input("docker-compose.yml").filename_like);
        assert!(!classify_input("找一下昨天改的 docker 配置").filename_like);
        // 句号结尾的中文句子不该被当成文件名
        assert!(!classify_input("我的项目文件夹在哪").filename_like);
    }

    #[test]
    fn short_words_do_not_match_inside_longer_words() {
        // 回归测试：`doc` 曾经子串命中 `docker`，导致纯关键词被当成
        // 「含类型词」送去调模型，闸门 1 直接失效。
        assert!(!classify_input("docker").has_type_word, "docker 不该命中 doc");
        assert!(!classify_input("vs").has_type_word);
        // 真正的类型词必须照常命中
        assert!(classify_input("找文档").has_type_word);
        assert!(classify_input("files I modified").has_type_word, "复数 files 应命中 file");
        assert!(classify_input("docs").has_type_word, "复数 docs 应命中 doc");
    }

    #[test]
    fn chinese_phrases_count_as_multiple_words() {
        // 回归测试：中文没有空格，若不折算词数，
        // 「我前几天弄的那个东西」会被当成单个孤立词、判不出自然语言。
        assert!(estimate_words("我前几天弄的那个东西") >= 3);
        assert_eq!(estimate_words("docker"), 1);
        assert_eq!(estimate_words("storage.rs"), 1);
        assert!(classify_input("我前几天弄的那个东西").is_natural);
    }

    #[test]
    fn intent_falls_back_gracefully_on_empty_answers() {
        let empty: Answers = HashMap::new();
        let it = Intent::from_answers(&empty, "test");
        // 缺失槽位回落到最宽松取值，且保守地认为「是搜索」
        assert_eq!(it.type_slot, TypeSlot::All);
        assert_eq!(it.time_slot, TimeSlot::Any);
        assert_eq!(it.location_slot, LocationSlot::Any);
        assert!(it.is_search, "缺失 is_search 时应保守认为在搜索");
        assert!(!it.is_natural);
    }

    #[test]
    fn confidence_reflects_slot_resolution_only() {
        // 什么都没解析出来 → 0，而不是被回落值拖住的 0.30
        let empty: Answers = HashMap::new();
        assert_eq!(Intent::from_answers(&empty, "t").confidence, 0.0);

        // 只解析出一个槽位 → 就是它的置信度
        let mut a: Answers = HashMap::new();
        a.insert(
            "type".into(),
            Answer::Choice {
                choice: "code".into(),
                confidence: 0.9,
                probabilities: HashMap::new(),
            },
        );
        assert!((Intent::from_answers(&a, "t").confidence - 0.9).abs() < 1e-6);

        // 多个槽位 → 取最低的那个
        a.insert(
            "time".into(),
            Answer::Choice {
                choice: "today".into(),
                confidence: 0.6,
                probabilities: HashMap::new(),
            },
        );
        assert!((Intent::from_answers(&a, "t").confidence - 0.6).abs() < 1e-6);
    }

    #[test]
    fn intent_reads_full_answers() {
        let mut a: Answers = HashMap::new();
        a.insert(
            "type".into(),
            Answer::Choice {
                choice: "code".into(),
                confidence: 0.96,
                probabilities: HashMap::new(),
            },
        );
        a.insert(
            "time".into(),
            Answer::Choice {
                choice: "yesterday".into(),
                confidence: 0.91,
                probabilities: HashMap::new(),
            },
        );
        a.insert("is_search".into(), Answer::Noul { noul: 0.98 });
        a.insert("is_natural".into(), Answer::Noul { noul: 0.93 });

        let it = Intent::from_answers(&a, "jev");
        assert_eq!(it.type_slot, TypeSlot::Code);
        assert_eq!(it.time_slot, TimeSlot::Yesterday);
        assert!(it.is_search);
        assert!(it.is_natural);
        assert_eq!(it.backend, "jev");
    }

    #[test]
    fn standard_questions_cover_all_slots() {
        let qs = standard_questions();
        let names: Vec<_> = qs.iter().map(|q| q.name).collect();
        assert_eq!(names, vec!["type", "time", "location", "is_search", "is_natural"]);
        // noul 不该带 criteria，choice 必须带
        for q in &qs {
            match q.kind {
                QuestionKind::Noul => assert!(matches!(q.criteria, Criteria::None)),
                QuestionKind::Choice => assert!(matches!(q.criteria, Criteria::Map(_))),
                QuestionKind::Score => assert!(matches!(q.criteria, Criteria::List(_))),
            }
        }
    }
}
