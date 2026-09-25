//! 查询埋点。
//!
//! **这是决定「后端默认值」与「是否值得上本地模型」的唯一依据**
//! （开发文档 §5.8 与 Phase 2 第 9 项都明确要求：不要靠猜）。
//!
//! 记录四件事：
//! 1. 查询的**语言分布**（拉丁 / CJK / 混合）—— 决定装哪个 checkpoint
//! 2. 是否自然语言 —— 决定闸门 2/3 的实际触发率
//! 3. 是否被规则解析出槽位 —— 决定规则版够不够用
//! 4. 走了哪级闸门 —— 决定云端成本
//!
//! ⚠️ **刻意不记录原始查询文本**。只做本地累计计数，
//! 避免把用户搜过什么写进磁盘 —— 需要细节时再单独开开关。
//!
//! 统计以 JSON 存在 `kv` 表里，每 [`FLUSH_EVERY`] 次查询落一次库，
//! 不阻塞搜索热路径。

use super::Lang;
use crate::core::storage::Storage;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

const KV_KEY: &str = "decision_telemetry";
/// 每累计这么多次查询落一次库 —— 避免每次搜索都写磁盘
const FLUSH_EVERY: u64 = 20;

/// 走的是哪一级闸门
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gate {
    /// 闸门 1：本地规则判定为纯关键词，**没有调模型**
    Rule,
    /// 闸门 2：本地模型
    Local,
    /// 闸门 3：云端
    Cloud,
}

impl Gate {
    pub fn as_str(self) -> &'static str {
        match self {
            Gate::Rule => "rule",
            Gate::Local => "local",
            Gate::Cloud => "cloud",
        }
    }
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Stats {
    pub total: u64,
    pub latin: u64,
    pub cjk: u64,
    pub mixed: u64,
    pub other: u64,
    pub natural: u64,
    /// 规则版至少解析出一个非「不限」槽位的次数
    pub slot_hit: u64,
    pub gate_rule: u64,
    pub gate_local: u64,
    pub gate_cloud: u64,
}

impl Stats {
    pub fn lang_pct(&self) -> Vec<(&'static str, f64)> {
        let n = self.total.max(1) as f64;
        vec![
            ("拉丁", self.latin as f64 * 100.0 / n),
            ("CJK", self.cjk as f64 * 100.0 / n),
            ("混合", self.mixed as f64 * 100.0 / n),
            ("其他", self.other as f64 * 100.0 / n),
        ]
    }

    /// 可读报告（设置页 / CLI 用）
    pub fn render(&self) -> String {
        if self.total == 0 {
            return "尚无查询样本".into();
        }
        let n = self.total as f64;
        let pct = |v: u64| v as f64 * 100.0 / n;
        let mut s = format!("共 {} 次查询\n", self.total);
        s.push_str("  语言分布：");
        s.push_str(
            &self
                .lang_pct()
                .iter()
                .map(|(k, v)| format!("{k} {v:.0}%"))
                .collect::<Vec<_>>()
                .join(" · "),
        );
        s.push_str(&format!(
            "\n  自然语言：{:.0}%（这些才需要模型）",
            pct(self.natural)
        ));
        s.push_str(&format!(
            "\n  规则解析出槽位：{:.0}%",
            pct(self.slot_hit)
        ));
        s.push_str(&format!(
            "\n  闸门分布：规则 {:.0}% · 本地模型 {:.0}% · 云端 {:.0}%",
            pct(self.gate_rule),
            pct(self.gate_local),
            pct(self.gate_cloud)
        ));
        s
    }
}

pub struct Telemetry {
    storage: Arc<Storage>,
    stats: Mutex<Stats>,
    since_flush: AtomicU64,
}

impl Telemetry {
    pub fn new(storage: Arc<Storage>) -> Self {
        let stats = storage
            .kv_get(KV_KEY)
            .and_then(|s| serde_json::from_str::<Stats>(&s).ok())
            .unwrap_or_default();
        Telemetry { storage, stats: Mutex::new(stats), since_flush: AtomicU64::new(0) }
    }

    /// 记一次查询。`slot_hit` 表示规则版解析出了非「不限」的槽位。
    pub fn record(&self, lang: Lang, is_natural: bool, slot_hit: bool, gate: Gate) {
        {
            let mut s = self.stats.lock().expect("telemetry poisoned");
            s.total += 1;
            match lang {
                Lang::Latin => s.latin += 1,
                Lang::Cjk => s.cjk += 1,
                Lang::Mixed => s.mixed += 1,
                Lang::Other => s.other += 1,
            }
            if is_natural {
                s.natural += 1;
            }
            if slot_hit {
                s.slot_hit += 1;
            }
            match gate {
                Gate::Rule => s.gate_rule += 1,
                Gate::Local => s.gate_local += 1,
                Gate::Cloud => s.gate_cloud += 1,
            }
        }
        // 攒够一批再落库，别让搜索热路径每次都写磁盘
        let n = self.since_flush.fetch_add(1, Ordering::Relaxed) + 1;
        if n >= FLUSH_EVERY {
            self.flush();
        }
    }

    pub fn snapshot(&self) -> Stats {
        self.stats.lock().expect("telemetry poisoned").clone()
    }

    pub fn render(&self) -> String {
        self.snapshot().render()
    }

    /// 落库。失败只记日志 —— 埋点绝不能影响主流程。
    pub fn flush(&self) {
        self.since_flush.store(0, Ordering::Relaxed);
        let snapshot = self.snapshot();
        match serde_json::to_string(&snapshot) {
            Ok(json) => {
                if let Err(e) = self.storage.kv_set(KV_KEY, &json) {
                    log::debug!("埋点落库失败（不影响主流程）：{e}");
                }
            }
            Err(e) => log::debug!("埋点序列化失败：{e}"),
        }
    }

    /// 清零（设置页「重置统计」用）
    pub fn reset(&self) {
        *self.stats.lock().expect("telemetry poisoned") = Stats::default();
        let _ = self.storage.kv_set(KV_KEY, "{}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_stats_render_is_readable() {
        let s = Stats::default();
        assert_eq!(s.render(), "尚无查询样本");
    }

    #[test]
    fn render_includes_all_dimensions() {
        let s = Stats {
            total: 10,
            cjk: 6,
            latin: 3,
            mixed: 1,
            natural: 4,
            slot_hit: 7,
            gate_rule: 8,
            gate_local: 2,
            ..Default::default()
        };
        let t = s.render();
        assert!(t.contains("共 10 次查询"), "{t}");
        assert!(t.contains("CJK 60%"), "{t}");
        assert!(t.contains("自然语言：40%"), "{t}");
        assert!(t.contains("规则解析出槽位：70%"), "{t}");
        assert!(t.contains("规则 80%"), "{t}");
    }

    #[test]
    fn lang_pct_sums_to_hundred() {
        let s = Stats { total: 4, latin: 1, cjk: 1, mixed: 1, other: 1, ..Default::default() };
        let sum: f64 = s.lang_pct().iter().map(|(_, v)| v).sum();
        assert!((sum - 100.0).abs() < 1e-6, "分布应合计 100%，实际 {sum}");
    }

    #[test]
    fn stats_json_roundtrip_tolerates_missing_fields() {
        // 老版本写下的 JSON 缺少新字段时不该解析失败
        let old = r#"{"total":3,"cjk":2}"#;
        let s: Stats = serde_json::from_str(old).unwrap();
        assert_eq!(s.total, 3);
        assert_eq!(s.cjk, 2);
        assert_eq!(s.latin, 0);
    }
}
