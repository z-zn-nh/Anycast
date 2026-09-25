//! 判断模型的槽位定义。
//!
//! ⚠️ **本文件里的选项表与 `tools/jev_probe.py` 逐字对齐，不要随手改措辞。**
//!
//! 开发文档 §2.1.2 记录了一条教训：模型、输入、参数全不动，**只改
//! `is_search` 的 `instructions` 措辞，准确率就从 77% 变成 96%**。
//! 也就是说措辞是这个功能的一部分，不是注释 —— 改字之前先重跑探针。
//!
//! 三个维度**分别作为一个 question 提问**，各自选项数远低于 20，
//! 正好落在模型的甜点区（合并成单次调用会显著掉准，见 §5.3）。

use chrono::{Datelike, Duration, Local, NaiveDate, TimeZone};

// ---------------------------------------------------------------------------
// 选项表
// ---------------------------------------------------------------------------

/// 类型维度（8 项）
pub const TYPE_OPTIONS: &[(&str, &str)] = &[
    ("document", "文档、笔记、说明、readme、md、pdf、word"),
    ("code", "源代码、脚本、配置、rs、py、js、json、yml、toml"),
    ("image", "图片、照片、截图、png、jpg、svg"),
    ("media", "音频、视频、mp3、mp4"),
    ("archive", "压缩包、zip、rar、7z"),
    ("executable", "可执行程序、exe、安装包"),
    ("folder", "文件夹、目录、项目根目录"),
    ("all", "无法判断类型，或用户不关心类型"),
];

/// 时间维度（8 项）
pub const TIME_OPTIONS: &[(&str, &str)] = &[
    ("today", "今天、刚刚、今天改的"),
    ("yesterday", "昨天"),
    ("this_week", "本周、这几天"),
    ("last_week", "上周"),
    ("this_month", "本月、这个月"),
    ("this_year", "今年"),
    ("older", "更早、很久以前"),
    ("any", "没有提到时间，或不限制时间"),
];

/// 位置维度（4 项）
pub const LOCATION_OPTIONS: &[(&str, &str)] = &[
    ("any", "没有提到位置"),
    ("current", "当前目录、这个文件夹里"),
    ("drive", "指定盘符，如 D 盘、E 盘"),
    ("common", "常用目录，如桌面、下载、文档、项目目录"),
];

// ---------------------------------------------------------------------------
// instructions（与探针逐字一致）
// ---------------------------------------------------------------------------

pub const INSTRUCTIONS_TYPE: &str = "用户想找的东西属于哪一类？";
pub const INSTRUCTIONS_TIME: &str = "用户是否限定了文件的时间范围？";
pub const INSTRUCTIONS_LOCATION: &str = "用户是否限定了查找的位置范围？";

/// 2026-09-24 首轮实测后改写。
///
/// 原措辞「是在搜索本机的文件或文件夹吗？」被模型理解成
/// 「这是不是一个明确的**搜索指令**」，导致 8/12 假阴性
/// （`docker` → 0.12、`我的项目文件夹在哪` → 0.29）。
/// 假阳性为 0，说明模型只是保守、不是判错。
/// 改为正面描述「在找东西」，并把「只给一个词或文件名」显式写入以消除歧义。
pub const INSTRUCTIONS_IS_SEARCH: &str = "用户是否在找本机的某个文件或文件夹？\
只要输入像是在找东西就算 —— 包括只给一个单词、一个文件名或一个名词短语。\
只有闲聊、问知识、要求生成内容才不算。";

/// 首轮实测：英文短语被系统性低估（0.42~0.49），中文都在 0.60 以上。
///
/// 原措辞「是自然语言句子吗」把**名词短语**排除在外，
/// 但 `the doc about storage logic` 恰恰是最典型的搜索输入。
/// 改为强调「描述」而非「句子」。
pub const INSTRUCTIONS_IS_NATURAL: &str = "用户是在用自然语言描述他想找的东西吗？\
整句、名词短语都算；只有一个孤立单词或纯文件名则不算。";

// ---------------------------------------------------------------------------
// 槽位枚举
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeSlot {
    Document,
    Code,
    Image,
    Media,
    Archive,
    Executable,
    Folder,
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeSlot {
    Today,
    Yesterday,
    ThisWeek,
    LastWeek,
    ThisMonth,
    ThisYear,
    Older,
    Any,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocationSlot {
    Any,
    Current,
    Drive,
    Common,
}

impl TypeSlot {
    pub fn as_str(self) -> &'static str {
        match self {
            TypeSlot::Document => "document",
            TypeSlot::Code => "code",
            TypeSlot::Image => "image",
            TypeSlot::Media => "media",
            TypeSlot::Archive => "archive",
            TypeSlot::Executable => "executable",
            TypeSlot::Folder => "folder",
            TypeSlot::All => "all",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        TYPE_OPTIONS
            .iter()
            .find(|(k, _)| *k == s)
            .map(|(k, _)| match *k {
                "document" => TypeSlot::Document,
                "code" => TypeSlot::Code,
                "image" => TypeSlot::Image,
                "media" => TypeSlot::Media,
                "archive" => TypeSlot::Archive,
                "executable" => TypeSlot::Executable,
                "folder" => TypeSlot::Folder,
                _ => TypeSlot::All,
            })
    }

    /// 映射到设置里已有的 `type_category`（供 `SearchScopeFilter` 直接消费）。
    pub fn to_scope_category(self) -> &'static str {
        match self {
            TypeSlot::Document => "document",
            TypeSlot::Code => "code",
            TypeSlot::Image => "image",
            TypeSlot::Media => "media",
            TypeSlot::Archive => "archive",
            TypeSlot::Executable => "executable",
            TypeSlot::Folder => "folder",
            TypeSlot::All => "all",
        }
    }

    pub fn label_cn(self) -> &'static str {
        match self {
            TypeSlot::Document => "文档",
            TypeSlot::Code => "代码",
            TypeSlot::Image => "图片",
            TypeSlot::Media => "音视频",
            TypeSlot::Archive => "压缩包",
            TypeSlot::Executable => "可执行",
            TypeSlot::Folder => "文件夹",
            TypeSlot::All => "全部",
        }
    }
}

impl TimeSlot {
    pub fn as_str(self) -> &'static str {
        match self {
            TimeSlot::Today => "today",
            TimeSlot::Yesterday => "yesterday",
            TimeSlot::ThisWeek => "this_week",
            TimeSlot::LastWeek => "last_week",
            TimeSlot::ThisMonth => "this_month",
            TimeSlot::ThisYear => "this_year",
            TimeSlot::Older => "older",
            TimeSlot::Any => "any",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        TIME_OPTIONS.iter().find(|(k, _)| *k == s).map(|(k, _)| match *k {
            "today" => TimeSlot::Today,
            "yesterday" => TimeSlot::Yesterday,
            "this_week" => TimeSlot::ThisWeek,
            "last_week" => TimeSlot::LastWeek,
            "this_month" => TimeSlot::ThisMonth,
            "this_year" => TimeSlot::ThisYear,
            "older" => TimeSlot::Older,
            _ => TimeSlot::Any,
        })
    }

    pub fn label_cn(self) -> &'static str {
        match self {
            TimeSlot::Today => "今天",
            TimeSlot::Yesterday => "昨天",
            TimeSlot::ThisWeek => "本周",
            TimeSlot::LastWeek => "上周",
            TimeSlot::ThisMonth => "本月",
            TimeSlot::ThisYear => "今年",
            TimeSlot::Older => "更早",
            TimeSlot::Any => "不限",
        }
    }

    /// 换算为 `[起, 止)` 的 unix 秒区间；`None` 表示该侧不设限。
    ///
    /// 用**本地时区**的零点做边界 —— 用户说的「昨天」是自己日历上的昨天。
    pub fn range(self) -> (Option<i64>, Option<i64>) {
        let now = Local::now();
        let today = now.date_naive();
        let midnight = |d: NaiveDate| -> i64 {
            Local
                .from_local_datetime(&d.and_hms_opt(0, 0, 0).expect("00:00:00 合法"))
                .single()
                .map(|dt| dt.timestamp())
                // 夏令时切换那一小时可能不存在，退化为当天 12:00 再取零点
                .unwrap_or_else(|| {
                    Local
                        .from_local_datetime(&d.and_hms_opt(12, 0, 0).expect("12:00:00 合法"))
                        .single()
                        .map(|dt| dt.timestamp() - 43_200)
                        .unwrap_or(0)
                })
        };
        // 周一为一周之始（ISO）
        let week_start = |d: NaiveDate| d - Duration::days(d.weekday().num_days_from_monday() as i64);

        match self {
            TimeSlot::Today => (Some(midnight(today)), None),
            TimeSlot::Yesterday => {
                let y = today - Duration::days(1);
                (Some(midnight(y)), Some(midnight(today)))
            }
            TimeSlot::ThisWeek => (Some(midnight(week_start(today))), None),
            TimeSlot::LastWeek => {
                let this_mon = week_start(today);
                let last_mon = this_mon - Duration::days(7);
                (Some(midnight(last_mon)), Some(midnight(this_mon)))
            }
            TimeSlot::ThisMonth => {
                let first = today.with_day(1).unwrap_or(today);
                (Some(midnight(first)), None)
            }
            TimeSlot::ThisYear => {
                let first = NaiveDate::from_ymd_opt(today.year(), 1, 1).unwrap_or(today);
                (Some(midnight(first)), None)
            }
            // 「更早」= 今年之前
            TimeSlot::Older => {
                let first = NaiveDate::from_ymd_opt(today.year(), 1, 1).unwrap_or(today);
                (None, Some(midnight(first)))
            }
            TimeSlot::Any => (None, None),
        }
    }
}

impl LocationSlot {
    pub fn as_str(self) -> &'static str {
        match self {
            LocationSlot::Any => "any",
            LocationSlot::Current => "current",
            LocationSlot::Drive => "drive",
            LocationSlot::Common => "common",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        LOCATION_OPTIONS.iter().find(|(k, _)| *k == s).map(|(k, _)| match *k {
            "current" => LocationSlot::Current,
            "drive" => LocationSlot::Drive,
            "common" => LocationSlot::Common,
            _ => LocationSlot::Any,
        })
    }

    pub fn label_cn(self) -> &'static str {
        match self {
            LocationSlot::Any => "不限",
            LocationSlot::Current => "当前目录",
            LocationSlot::Drive => "指定盘符",
            LocationSlot::Common => "常用目录",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn option_tables_stay_within_sweet_spot() {
        // 每个维度都必须远低于 20 —— 超了会显著掉准（§5.3）
        assert!(TYPE_OPTIONS.len() < 20);
        assert!(TIME_OPTIONS.len() < 20);
        assert!(LOCATION_OPTIONS.len() < 20);
    }

    #[test]
    fn parse_roundtrip() {
        for (k, _) in TYPE_OPTIONS {
            assert_eq!(TypeSlot::parse(k).unwrap().as_str(), *k);
        }
        for (k, _) in TIME_OPTIONS {
            assert_eq!(TimeSlot::parse(k).unwrap().as_str(), *k);
        }
        for (k, _) in LOCATION_OPTIONS {
            assert_eq!(LocationSlot::parse(k).unwrap().as_str(), *k);
        }
        assert!(TypeSlot::parse("不存在").is_none());
    }

    #[test]
    fn time_ranges_are_ordered_and_sane() {
        let (a, b) = TimeSlot::Yesterday.range();
        let a = a.expect("昨天有下界");
        let b = b.expect("昨天有上界");
        assert_eq!(b - a, 86_400, "昨天恰好 24 小时");

        let (tw, _) = TimeSlot::ThisWeek.range();
        let (lw, lwe) = TimeSlot::LastWeek.range();
        assert!(lw.unwrap() < tw.unwrap(), "上周起点早于本周起点");
        // 上周是半开区间 [上周一, 本周一)，所以它的终点正好是本周的起点
        assert_eq!(lwe.unwrap(), tw.unwrap(), "上周终点 = 本周起点（半开区间）");
        assert_eq!(tw.unwrap() - lw.unwrap(), 86_400 * 7, "上周恰好 7 天");

        assert_eq!(TimeSlot::Any.range(), (None, None));

        // 「更早」的上界 = 今年起点，且必须是正数（1970 之后的合理时间）
        let (_, older_end) = TimeSlot::Older.range();
        assert!(older_end.unwrap() > 1_600_000_000);
    }
}
