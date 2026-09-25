//! 槽位 → 检索范围（`SearchScopeFilter`）。
//!
//! 这一层是「判断模型」与「检索」之间**唯一**的耦合点：
//! 模型只输出枚举，把枚举翻译成 SQL 过滤条件的活全在这里。
//! 模型本身不认识 `type_category` / `time_preset` 这些检索侧概念，
//! 所以**不要在 `slots.rs` 或后端实现里做这个翻译**。
//!
//! # 优先级
//!
//! ```text
//! 用户手点的筛选条件  >  判断模型的槽位  >  search::parse_intent 的词表
//! ```
//!
//! 桥接只填**空**的维度。理由：用户显式说了「只看代码」，
//! 模型再根据「文档」两个字把范围改成 document，是在跟用户对着干。
//!
//! # 与 `search::parse_intent` 的关系
//!
//! 两者都做意图解析，但分工不同：
//! - `parse_intent` 产出**关键词**（模型被明令禁止生成关键词，见模块头注释）
//! - 本桥接产出**筛选范围**
//!
//! 重合的维度（类型 / 时间）上，模型的槽位更精确（ISO 周、精确的月/年边界、
//! `image` 与 `media` 的区分），所以**模型先落，`parse_intent` 只补它没覆盖的**
//! —— 后者独有的 `clipboard` / `app` 类型、`最近` / `近期` 这类相对时间仍然生效。

use crate::models::{IntentChip, SearchScopeFilter};

use super::slots::{LocationSlot, TypeSlot};
use super::Intent;

/// 把槽位结论落到检索范围上，返回新增的意图芯片。
///
/// 纯函数：不改 `intent`，只改 `scope`。返回的芯片供 UI 显示
/// 「这次是按什么筛的」—— 用户得能看见模型擅自加了什么条件。
pub fn apply(intent: &Intent, scope: &mut SearchScopeFilter) -> Vec<IntentChip> {
    let mut chips = Vec::new();

    if is_unset(&scope.type_category) {
        if let Some(cat) = map_type(intent.type_slot) {
            scope.type_category = cat.to_string();
            chips.push(chip("类型", intent.type_slot.label_cn()));
        }
    }

    if is_unset(&scope.time_preset) {
        // `range()` 给的是**排他**上界（下一天零点）。
        let (lo, hi) = intent.time_slot.range();
        if lo.is_some() || hi.is_some() {
            scope.time_preset = "range".into();
            scope.custom_start_time = lo;
            // ⚠️ `SearchScopeFilter::time_upper_bound` 的约定是
            // 「**含**当天 23:59:59」，所以排他上界要减 1 秒。
            // 不减的话「昨天」会连今天一起返回（这个 off-by-one 曾经真的存在）。
            scope.custom_end_time = hi.map(|e| e - 1);
            chips.push(chip("时间", intent.time_slot.label_cn()));
        }
    }

    if is_unset(&scope.location_scope) {
        match intent.location_slot {
            // `Current` 不给范围键：面板是浮动的，无头状态下无从得知
            // 「当前目录」是哪个（UI 里靠用户手选目录来表达）。
            // 编一个出来只会把结果筛成空 —— 那比不筛更糟。
            LocationSlot::Any | LocationSlot::Current => {}
            LocationSlot::Drive | LocationSlot::Common => {
                if let Some(key) = intent.location_scope.as_deref() {
                    scope.location_scope = key.to_string();
                    chips.push(chip("位置", intent.location_slot.label_cn()));
                }
            }
        }
    }

    chips
}

fn chip(key: &str, val: &str) -> IntentChip {
    IntentChip { key: key.into(), val: val.into() }
}

fn is_unset(v: &str) -> bool {
    v.is_empty() || v == "all"
}

/// `TypeSlot` → `SearchScopeFilter::type_category`。
///
/// `All` 返回 `None`（不改范围）。其余直接复用 `to_scope_category()`，
/// 它给出的每个取值都必须在 `storage::type_extensions` 或 `item_in_scope`
/// 里有对应分支 —— 否则会**静默不过滤**（`unwrap_or(true)`），
/// 看着像筛过了，其实一条没筛。`image` / `executable` 就是为此补进
/// `type_extensions` 的。
fn map_type(slot: TypeSlot) -> Option<&'static str> {
    match slot {
        TypeSlot::All => None,
        other => Some(other.to_scope_category()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::decision::rule::RuleBackend;
    use crate::core::decision::{standard_questions, DecisionBackend};
    use crate::models::SearchMode;

    fn intent(q: &str) -> Intent {
        let b = RuleBackend::new();
        let a = b.decide(q, &standard_questions()).unwrap();
        Intent::from_answers(&a, b.id())
    }

    #[test]
    fn type_slot_lands_on_scope_category() {
        let mut scope = SearchScopeFilter::default();
        let chips = apply(&intent("找一下昨天改的 docker 配置"), &mut scope);
        assert_eq!(scope.type_category, "code");
        assert!(chips.iter().any(|c| c.key == "类型"));
    }

    /// `image` 必须落在 `image` 而不是 `media` —— 否则「找图片」会把视频也带出来。
    /// 同时 `image` 必须在 `type_extensions` 里有分支（见 `map_type` 注释）。
    #[test]
    fn image_and_media_stay_apart() {
        let mut scope = SearchScopeFilter::default();
        apply(&intent("找几张截图"), &mut scope);
        assert_eq!(scope.type_category, "image");
        assert!(crate::core::storage::type_extensions("image").is_some());

        let mut scope = SearchScopeFilter::default();
        apply(&intent("找点视频"), &mut scope);
        assert_eq!(scope.type_category, "media");
    }

    /// `executable` 也得有扩展名表，否则「找安装包」等于没筛。
    #[test]
    fn executable_category_is_filterable() {
        assert!(crate::core::storage::type_extensions("executable").is_some());
        let mut scope = SearchScopeFilter::default();
        apply(&intent("找一下安装包"), &mut scope);
        assert_eq!(scope.type_category, "executable");
    }

    #[test]
    fn time_slot_becomes_a_closed_range() {
        let mut scope = SearchScopeFilter::default();
        apply(&intent("昨天改的"), &mut scope);
        assert_eq!(scope.time_preset, "range");
        let lo = scope.custom_start_time.expect("起点");
        let hi = scope.custom_end_time.expect("终点");
        // 恰好一天：含头含尾
        assert_eq!(hi - lo, 86_399, "「昨天」必须正好是一天");
        // 且 `time_upper_bound` 原样返回，不再补一天
        assert_eq!(scope.time_upper_bound(), Some(hi));
    }

    /// 开放区间（今天 / 本周 / 今年）只给下界，不给上界 ——
    /// 给了上界会把「现在之后」的时间排除掉，那是错的。
    #[test]
    fn open_ended_slots_have_no_upper_bound() {
        let mut scope = SearchScopeFilter::default();
        apply(&intent("今天改的"), &mut scope);
        assert!(scope.custom_start_time.is_some());
        assert!(scope.custom_end_time.is_none());
        assert_eq!(scope.time_upper_bound(), None);
    }

    #[test]
    fn drive_letter_becomes_a_concrete_scope_key() {
        let mut scope = SearchScopeFilter::default();
        let chips = apply(&intent("D盘那个配置"), &mut scope);
        assert_eq!(scope.location_scope, "drive-d");
        assert!(chips.iter().any(|c| c.key == "位置"));
    }

    #[test]
    fn common_dir_becomes_a_concrete_scope_key() {
        let mut scope = SearchScopeFilter::default();
        apply(&intent("桌面上那个文档"), &mut scope);
        assert_eq!(scope.location_scope, "desktop");

        let mut scope = SearchScopeFilter::default();
        apply(&intent("下载的压缩包"), &mut scope);
        assert_eq!(scope.location_scope, "downloads");
    }

    /// 「当前目录」不给范围键 —— 给了就会把结果筛空。
    #[test]
    fn current_dir_is_left_alone() {
        let mut scope = SearchScopeFilter::default();
        apply(&intent("这个文件夹里的东西"), &mut scope);
        assert_eq!(scope.location_scope, "", "不得凭空编出一个范围");
    }

    /// 用户手点的筛选条件优先，模型不许覆盖。
    #[test]
    fn user_scope_wins() {
        let mut scope = SearchScopeFilter {
            type_category: "folder".into(),
            time_preset: "today".into(),
            location_scope: "drive-c".into(),
            ..Default::default()
        };
        let chips = apply(&intent("D盘昨天改的 docker 配置"), &mut scope);
        assert_eq!(scope.type_category, "folder");
        assert_eq!(scope.time_preset, "today");
        assert_eq!(scope.location_scope, "drive-c");
        assert!(chips.is_empty(), "全被用户占着时不该报「模型筛了什么」");
    }

    /// 什么都没解析出来的查询不该改动范围（纯关键词路径）。
    #[test]
    fn plain_keyword_changes_nothing() {
        let mut scope = SearchScopeFilter::default();
        let chips = apply(&intent("docker"), &mut scope);
        assert!(chips.is_empty());
        assert!(!scope.is_active());
    }

    #[test]
    fn mode_enum_is_not_confused() {
        // 桥接与搜索模式无关，这里只是钉住 `SearchMode` 的默认值没变
        assert_eq!(SearchMode::default(), SearchMode::Fast);
    }
}
