//! 三级闸门的调度中心。
//!
//! ```text
//! 输入
//!  ├─ 闸门 1 · 本地规则（<1ms，零成本）
//!  │    纯关键词 → 直接检索，【不调模型】        ← DecisionHub::analyze
//!  ├─ 闸门 2 · 本地模型（200~460ms，零成本）
//!  │    先出本地结果 → 异步解析槽位 → 二次检索   ← DecisionHub::upgrade
//!  └─ 闸门 3 · 云端 Jev（p50 ≈ 900ms）
//!       仅显式开启且低置信时；硬超时 2000ms       ← DecisionHub::upgrade
//! ```
//!
//! **三条硬性规则**（§5.7）：
//! 1. 首屏永远由本地检索提供 —— [`analyze`](DecisionHub::analyze) 是同步且 < 1 ms 的，
//!    可以直接在 UI 线程调；[`upgrade`](DecisionHub::upgrade) 会阻塞，**必须**放后台线程。
//! 2. 任何一级失败或超时 → 静默降级到上一级，**不弹错误提示**。
//! 3. 纯关键词输入永不触发闸门 2 / 3。

use super::jev::JevBackend;
use super::rule::RuleBackend;
use super::telemetry::{Gate, Telemetry};
use super::{classify_input, DecisionBackend, InputShape, Intent};
use crate::core::settings::AppSettings;
use crate::core::storage::Storage;
use anyhow::Result;
use std::sync::Arc;

/// 闸门 1 的产物：**永远可用**，作为兜底结果。
#[derive(Debug, Clone)]
pub struct Analysis {
    pub shape: InputShape,
    /// 规则版结论（即使后面升级到云端，它也先给用户一个结果）
    pub intent: Intent,
    /// 规则是否解析出了至少一个有效槽位。
    ///
    /// 这是「要不要花钱调云端」的实际判据 —— 规则已经认出来了就别再问模型。
    /// 真正需要云端的是「我前几天弄的那个东西」这类**词表覆盖不到**的说法：
    /// 规则一个槽位都解析不出来，而云端能听懂「前几天」。
    pub slot_hit: bool,
    /// 建议是否继续调用模型（闸门 2 / 3）
    pub worth_upgrade: bool,
    /// 本次实际走到的闸门（用于埋点）
    pub gate: Gate,
}

pub struct DecisionHub {
    rule: RuleBackend,
    telemetry: Telemetry,
}

impl DecisionHub {
    pub fn new(storage: Arc<Storage>) -> Self {
        DecisionHub { rule: RuleBackend::new(), telemetry: Telemetry::new(storage) }
    }

    pub fn telemetry(&self) -> &Telemetry {
        &self.telemetry
    }

    /// **闸门 1**：同步、< 1 ms、可安全在 UI 线程调用。
    ///
    /// 顺手记埋点 —— 语言分布、是否自然语言、规则有没有解析出槽位，
    /// 这三项是决定后端默认值的唯一依据（§5.8）。
    pub fn analyze(&self, query: &str, settings: &AppSettings) -> Analysis {
        let a = self.analyze_inner(query, settings);
        // 开关没开时不计入埋点（用户没启用这个功能，统计里不该出现）
        if settings.ai_enabled && settings.ai_intent_parsing {
            self.telemetry
                .record(a.shape.lang, a.shape.is_natural, a.slot_hit, a.gate);
        }
        a
    }

    /// 不记埋点的版本 —— [`upgrade`](Self::upgrade) 内部复用，
    /// 否则一次查询会被记两次（调用方本就会先调 `analyze`）。
    fn analyze_inner(&self, query: &str, settings: &AppSettings) -> Analysis {
        let shape = classify_input(query);

        // 开关关掉时直接给「不升级」的规则结论
        if !settings.ai_enabled || !settings.ai_intent_parsing {
            let intent = self.rule_intent(query);
            return Analysis {
                shape,
                intent,
                slot_hit: false,
                worth_upgrade: false,
                gate: Gate::Rule,
            };
        }

        let intent = self.rule_intent(query);
        let slot_hit = intent.has_slot();

        let worth_upgrade = shape.worth_model();
        // 埋点里的 gate 表示「如果没有更高级后端可用，这次会停在哪」
        let gate = if !worth_upgrade {
            Gate::Rule
        } else if self.cloud_ready(settings) {
            Gate::Cloud
        } else {
            Gate::Local
        };

        Analysis { shape, intent, slot_hit, worth_upgrade, gate }
    }

    /// **闸门 2 / 3**：阻塞调用（云端硬超时 2000 ms）。
    ///
    /// 返回 `Ok(None)` 表示「无需升级」或「没有可用后端」——
    /// 调用方**照常使用 `analyze()` 的规则结论**，不要当作失败。
    /// 返回 `Err` 表示升级失败，同样静默降级（§5.7 硬性规则 2）。
    pub fn upgrade(&self, query: &str, settings: &AppSettings) -> Result<Option<Intent>> {
        if !settings.ai_enabled || !settings.ai_intent_parsing {
            return Ok(None);
        }
        let analysis = self.analyze_inner(query, settings);
        if !analysis.worth_upgrade {
            return Ok(None);
        }
        // 规则已经解析出至少一个槽位时不必花钱调云端 —— 除非用户明确选了 jev 模式
        if analysis.slot_hit && !self.force_cloud(settings) {
            return Ok(None);
        }
        let Some(backend) = self.cloud_backend(settings) else {
            return Ok(None);
        };
        let answers = backend.decide(query, &super::standard_questions())?;
        Ok(Some(Intent::from_answers(&answers, backend.id())))
    }

    /// 云端后端是否配置齐全且被允许使用
    pub fn cloud_ready(&self, settings: &AppSettings) -> bool {
        self.cloud_backend(settings).is_some()
    }

    /// 设置页「云端连接」那一组的状态文案：`(说明, 徽标文字, 是否就绪)`。
    ///
    /// 关键是把「为什么没生效」直接写出来 —— 最常见的两种情况是
    /// 「模式不允许联网」和「模式开了但没填 Key」，它们在界面上长得一模一样，
    /// 用户只会以为功能坏了。
    ///
    /// 判据走 [`cloud_backend`](Self::cloud_backend) 本身，而不是在这里重写一遍
    /// 那套 `allowed` / `resolve_credentials` 逻辑 —— 否则界面说的和实际做的会漂移，
    /// 那比不显示状态更糟。
    pub fn cloud_status(&self, settings: &AppSettings) -> (String, String, bool) {
        let mode = settings.ai_backend_mode.as_str();
        let allowed = match mode {
            "jev" => true,
            "auto" => settings.ai_cloud_fallback,
            _ => false,
        };
        if !allowed {
            let why = if mode == "rule" {
                "已选「纯本地规则」，不会联网"
            } else {
                "「自动」模式下还需打开「低置信时升级到云端」"
            };
            return (why.to_string(), "○ 未启用".into(), false);
        }
        let key = JevBackend::resolve_credentials(&settings.ai_jev_endpoint, &settings.ai_jev_api_key);
        if key.trim().is_empty() {
            return (
                "已启用云端，但没填 API Key —— 每次查询都会静默跳过模型".into(),
                "⚠ 缺 Key".into(),
                false,
            );
        }
        // Key 从环境变量来的时候配置里是空的，说清楚免得用户以为没保存上
        let from_env = if settings.ai_jev_api_key.trim().is_empty() { "（Key 来自环境变量）" } else { "" };
        (
            format!("{}{} · 模型 {}", settings.ai_jev_endpoint.trim(), from_env, settings.ai_jev_model),
            "● 就绪".into(),
            true,
        )
    }

    /// 「本地推理后端」那一行：当前**实际**在用哪一级。
    ///
    /// 与 [`cloud_status`](Self::cloud_status) 分开，是因为这一行讲的是
    /// 「查询会被怎么处理」，而那一组讲的是「云端连不连得上」。
    pub fn backend_status_line(&self, settings: &AppSettings) -> String {
        let cloud = self.cloud_backend(settings).is_some();
        match (settings.ai_backend_mode.as_str(), cloud) {
            ("rule", _) => "规则意图解析 + FTS5 正文检索 · 零网络请求".into(),
            ("jev", true) => format!("云端 Jev（{}）· 强制跳过规则", settings.ai_jev_model),
            ("jev", false) => "选了云端 Jev，但云端未就绪 → 已静默降级为规则".into(),
            ("auto", true) => "规则优先；规则没把握时才升级到云端 Jev".into(),
            ("auto", false) => "规则意图解析 + FTS5 正文检索 · 零网络请求".into(),
            (other, _) => format!("规则意图解析（未识别的模式 {other:?}，按规则处理）"),
        }
    }

    /// 按设置决定云端后端；不可用返回 None（调用方静默降级）
    fn cloud_backend(&self, settings: &AppSettings) -> Option<JevBackend> {
        let mode = settings.ai_backend_mode.as_str();
        // `rule` 明确表示不联网；`auto` 需要用户额外打开云端回退
        let allowed = match mode {
            "jev" => true,
            "auto" => settings.ai_cloud_fallback,
            _ => false,
        };
        if !allowed {
            return None;
        }
        let key = JevBackend::resolve_credentials(&settings.ai_jev_endpoint, &settings.ai_jev_api_key);
        if key.trim().is_empty() {
            log::debug!("云端后端已启用但未配置 API Key，静默跳过");
            return None;
        }
        let proxy = if settings.ai_jev_proxy.trim().is_empty() {
            None
        } else {
            Some(settings.ai_jev_proxy.trim().to_string())
        };
        Some(
            JevBackend::new(
                settings.ai_jev_endpoint.clone(),
                key,
                settings.ai_jev_model.clone(),
            )
            .with_proxy(proxy),
        )
    }

    /// 模式显式选了 `jev` 时，即使规则有把握也走云端（用户明确要求）
    fn force_cloud(&self, settings: &AppSettings) -> bool {
        settings.ai_backend_mode == "jev"
    }

    fn rule_intent(&self, query: &str) -> Intent {
        match self.rule.decide(query, &[]) {
            Ok(a) => Intent::from_answers(&a, self.rule.id()),
            // 规则后端不会失败，真失败了也不能让搜索挂掉
            Err(e) => {
                log::warn!("规则后端异常：{e}");
                Intent::from_answers(&super::Answers::new(), "rule")
            }
        }
    }

    /// 设置页 / CLI 用的可读统计
    pub fn telemetry_report(&self) -> String {
        self.telemetry.render()
    }

    pub fn flush_telemetry(&self) {
        self.telemetry.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings_with(mode: &str, cloud: bool, key: &str) -> AppSettings {
        AppSettings {
            ai_enabled: true,
            ai_intent_parsing: true,
            ai_backend_mode: mode.into(),
            ai_cloud_fallback: cloud,
            ai_jev_api_key: key.into(),
            // 测试绝不打真实端点：万一某条断言回归、真的走到联网那一步，
            // 也该打在本地死端口上立刻失败，而不是把请求发出去烧钱。
            ai_jev_endpoint: "http://127.0.0.1:1/v1/systemone".into(),
            ..Default::default()
        }
    }

    // 这些用例只碰闸门 1 与配置判定，不发起任何网络请求。
    // 用内存库构造 hub —— 埋点会写 kv 表，所以需要真实 Storage。
    // 每个用例用独立库文件，避免 cargo 并行跑测试时撞 SQLite 锁。
    fn hub(name: &str) -> DecisionHub {
        let dir = std::env::temp_dir().join(format!("anycast_hub_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).ok();
        let storage = Arc::new(Storage::open(&dir.join(format!("{name}.db"))).expect("open test db"));
        DecisionHub::new(storage)
    }

    #[test]
    fn keywords_stop_at_gate_one() {
        let h = hub("keywords_stop_at_gate_one");
        let s = settings_with("auto", true, "k");
        let a = h.analyze("docker", &s);
        assert!(!a.worth_upgrade, "纯关键词不该升级");
        assert_eq!(a.gate, Gate::Rule);
    }

    #[test]
    fn natural_language_proposes_upgrade() {
        let h = hub("natural_language_proposes_upgrade");
        let s = settings_with("auto", true, "k");
        let a = h.analyze("找一下昨天改的 docker 配置", &s);
        assert!(a.worth_upgrade);
        // 开了云端回退 → 埋点记为 Cloud
        assert_eq!(a.gate, Gate::Cloud);
    }

    #[test]
    fn cloud_not_ready_without_key() {
        let h = hub("cloud_not_ready_without_key");
        // mode=jev 但没 key → 不算就绪
        assert!(!h.cloud_ready(&settings_with("jev", false, "")));
        // mode=auto 但没开回退 → 不算就绪
        assert!(!h.cloud_ready(&settings_with("auto", false, "k")));
        // mode=rule 即使有 key 也不联网
        assert!(!h.cloud_ready(&settings_with("rule", true, "k")));
        // 两个条件都满足才就绪
        assert!(h.cloud_ready(&settings_with("auto", true, "k")));
        assert!(h.cloud_ready(&settings_with("jev", false, "k")));
    }

    #[test]
    fn upgrade_is_noop_for_keywords() {
        let h = hub("upgrade_is_noop_for_keywords");
        let s = settings_with("auto", true, "k");
        // 关键词 → Ok(None)，且不会发起网络请求
        assert!(h.upgrade("docker", &s).unwrap().is_none());
    }

    #[test]
    fn disabled_ai_never_upgrades() {
        let h = hub("disabled_ai_never_upgrades");
        let mut s = settings_with("jev", false, "k");
        s.ai_enabled = false;
        let a = h.analyze("找一下昨天改的 docker 配置", &s);
        assert!(!a.worth_upgrade);
        assert!(h.upgrade("找一下昨天改的 docker 配置", &s).unwrap().is_none());

        s.ai_enabled = true;
        s.ai_intent_parsing = false;
        assert!(h.upgrade("找一下昨天改的 docker 配置", &s).unwrap().is_none());
    }

    #[test]
    fn rule_slot_hit_skips_cloud() {
        let h = hub("rule_slot_hit_skips_cloud");
        let s = settings_with("auto", true, "k");
        // 规则命中类型+时间 → slot_hit → 不升级（因此不会联网）
        let q = "找一下昨天改的 docker 配置";
        assert!(h.upgrade(q, &s).unwrap().is_none(), "规则已解析出槽位时不该调云端");
    }

    #[test]
    fn unmatched_phrasing_is_what_cloud_is_for() {
        let h = hub("unmatched_phrasing_is_what_cloud_is_for");
        let s = settings_with("auto", true, "k");
        // 词表覆盖不到的说法：规则解析不出任何槽位，这才是云端该出场的场合。
        // 这里只断言闸门 1 的判定，不真的发请求（upgrade 会尝试联网，故不调用）。
        let a = h.analyze("我前几天弄的那个东西", &s);
        assert!(a.worth_upgrade, "自然语言应提议升级");
        assert!(!a.slot_hit, "「前几天」不在规则词表里，不该解析出槽位");
    }
}
