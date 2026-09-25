//! Jev 云端后端（TypeSafe System One）。
//!
//! # 为什么用 WinHTTP 而不是 reqwest/ureq
//!
//! 1. **零新增依赖** —— 项目已有 `windows` crate，只是多开一个 feature；
//!    引 rustls/native-tls 会让二进制和编译时间都明显变大。
//! 2. **自动跟随系统代理** —— `WINHTTP_ACCESS_TYPE_DEFAULT_PROXY` 读的就是
//!    IE/系统代理设置，Clash Verge 开「系统代理」时直接生效，用户不必再配一遍。
//!    需要显式代理时用 `WINHTTP_ACCESS_TYPE_NAMED_PROXY`。
//!
//! # 必须遵守的三条前提（开发文档 §2.1.2）
//!
//! 1. `instructions` 用 [`super::slots`] 里**校准过**的措辞 —— 旧措辞只有 77%
//! 2. **异步调用 + 2000 ms 硬超时 + 首屏不等**。实测 p50 约 900 ms、
//!    min 约 750 ms、首轮冷启动 15 s，所以「即时」不可能
//! 3. **默认关闭**，由用户显式开启
//!
//! ⚠️ [`JevBackend::decide`] 是**同步阻塞**的。调用方必须放到后台线程，
//! 且拿到结果后只做**增量替换**，不得清空已渲染的列表（§5.7 硬性规则 1）。

use super::{Answer, Answers, Criteria, DecisionBackend, Question, QuestionKind};
use anyhow::{anyhow, bail, Result};
use std::collections::HashMap;
use windows::core::PCWSTR;
use windows::Win32::Networking::WinHttp::*;

/// 官方端点（TypeSafe 排队申请，key 前缀 `ts_`）
pub const OFFICIAL_ENDPOINT: &str = "https://api.typesafe.ai/v1/systemone";
/// 第三方托管端点（`jv_live_` 前缀，免排队）
pub const HOSTED_ENDPOINT: &str = "https://jevtypesafeai.com/api/v1/decide";

/// 官方 SDK 读的环境变量名（按端点区分，与探针一致）
pub const ENV_OFFICIAL: &str = "TYPESAFE_API_KEY";
pub const ENV_HOSTED: &str = "JEV_API_KEY";

pub const DEFAULT_MODEL: &str = "jev-latest";

/// 硬超时。实测 p50 已到 900 ms，800 ms 会砍掉一半请求（§5.7 实测修正）。
pub const TIMEOUT_MS: u32 = 2000;

pub struct JevBackend {
    endpoint: String,
    api_key: String,
    model: String,
    /// 显式代理（如 `http://127.0.0.1:7897`）；`None` 表示跟随系统代理
    proxy: Option<String>,
    timeout_ms: u32,
}

impl JevBackend {
    pub fn new(endpoint: impl Into<String>, api_key: impl Into<String>, model: impl Into<String>) -> Self {
        JevBackend {
            endpoint: endpoint.into(),
            api_key: api_key.into(),
            model: model.into(),
            proxy: None,
            timeout_ms: TIMEOUT_MS,
        }
    }

    pub fn with_proxy(mut self, proxy: Option<String>) -> Self {
        self.proxy = proxy.filter(|p| !p.trim().is_empty());
        self
    }

    pub fn with_timeout(mut self, ms: u32) -> Self {
        self.timeout_ms = ms;
        self
    }

    /// 按端点选默认环境变量名
    pub fn env_name_for(endpoint: &str) -> &'static str {
        if endpoint.contains("typesafe.ai") && !endpoint.contains("jevtypesafeai") {
            ENV_OFFICIAL
        } else {
            ENV_HOSTED
        }
    }

    /// 端点 + Key 的解析顺序：**环境变量优先于配置文件**。
    ///
    /// 理由：Key 写在 `config.json` 里是明文，环境变量至少不落盘。
    /// 用户两种都配了就优先用环境变量。
    pub fn resolve_credentials(endpoint: &str, cfg_key: &str) -> String {
        let env_name = Self::env_name_for(endpoint);
        std::env::var(env_name)
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| cfg_key.trim().to_string())
    }
}

impl DecisionBackend for JevBackend {
    fn id(&self) -> &'static str {
        "jev"
    }

    fn display_name(&self) -> &'static str {
        "云端 Jev"
    }

    fn is_local(&self) -> bool {
        false
    }

    fn is_available(&self) -> bool {
        !self.api_key.trim().is_empty() && !self.endpoint.trim().is_empty()
    }

    fn warm(&self) -> Result<()> {
        if !self.is_available() {
            bail!("Jev 未配置 API Key");
        }
        // 首轮冷启动实测 15 s —— 预热一次把连接与模型热起来。
        // 用最小请求，代价约 $0.00003。
        let body = build_body("warmup", &[], &self.model);
        let _ = http_post(&self.endpoint, &self.api_key, &body, self.proxy.as_deref(), 30_000)?;
        Ok(())
    }

    fn decide(&self, state: &str, questions: &[Question]) -> Result<Answers> {
        if !self.is_available() {
            bail!("Jev 未配置 API Key");
        }
        let body = build_body(state, questions, &self.model);
        let text = http_post(
            &self.endpoint,
            &self.api_key,
            &body,
            self.proxy.as_deref(),
            self.timeout_ms,
        )?;
        parse_answers(&text)
    }
}

// ---------------------------------------------------------------------------
// 请求体构造
// ---------------------------------------------------------------------------

/// 把问题列表编成请求 JSON。
///
/// **一次调用发全部问题** —— 官方明确建议批量（并行执行、共享 state 成本），
/// 拆成多次既慢又贵。
pub fn build_body(state: &str, questions: &[Question], model: &str) -> String {
    let mut qs = serde_json::Map::new();
    for q in questions {
        let mut obj = serde_json::Map::new();
        obj.insert(
            "type".into(),
            serde_json::Value::String(
                match q.kind {
                    QuestionKind::Choice => "choice",
                    QuestionKind::Score => "score",
                    QuestionKind::Noul => "noul",
                }
                .into(),
            ),
        );
        obj.insert(
            "instructions".into(),
            serde_json::Value::String(q.instructions.into()),
        );
        // ⚠️ 三种类型的 criteria 写法不同（§2.1.1）：
        //    choice → 映射；score → 有序数组；noul → 不传
        match &q.criteria {
            Criteria::Map(m) => {
                let map: serde_json::Map<String, serde_json::Value> = m
                    .iter()
                    .map(|(k, v)| ((*k).to_string(), serde_json::Value::String((*v).into())))
                    .collect();
                obj.insert("criteria".into(), serde_json::Value::Object(map));
            }
            Criteria::List(l) => {
                let arr: Vec<serde_json::Value> =
                    l.iter().map(|s| serde_json::Value::String((*s).into())).collect();
                obj.insert("criteria".into(), serde_json::Value::Array(arr));
            }
            Criteria::None => {}
        }
        qs.insert(q.name.into(), serde_json::Value::Object(obj));
    }

    serde_json::json!({
        "model": model,
        "state": state,
        "questions": serde_json::Value::Object(qs),
    })
    .to_string()
}

// ---------------------------------------------------------------------------
// 响应解析
// ---------------------------------------------------------------------------

pub fn parse_answers(text: &str) -> Result<Answers> {
    let v: serde_json::Value =
        serde_json::from_str(text).map_err(|e| anyhow!("Jev 响应不是合法 JSON：{e}"))?;

    let answers = v
        .get("answers")
        .and_then(|a| a.as_object())
        .ok_or_else(|| anyhow!("Jev 响应缺少 answers 字段：{}", truncate(text, 200)))?;

    let mut out = Answers::new();
    for (name, a) in answers {
        let kind = a.get("type").and_then(|t| t.as_str()).unwrap_or("");
        let parsed = match kind {
            "choice" => {
                let choice = a
                    .get("choice")
                    .and_then(|c| c.as_str())
                    .ok_or_else(|| anyhow!("choice 答案缺少 choice 字段"))?
                    .to_string();
                let confidence = a.get("confidence").and_then(|c| c.as_f64()).unwrap_or(0.0) as f32;
                let mut probabilities = HashMap::new();
                if let Some(p) = a.get("probabilities").and_then(|p| p.as_object()) {
                    for (k, val) in p {
                        probabilities.insert(k.clone(), val.as_f64().unwrap_or(0.0) as f32);
                    }
                }
                Answer::Choice { choice, confidence, probabilities }
            }
            "score" => Answer::Score {
                score: a.get("score").and_then(|s| s.as_f64()).unwrap_or(0.0) as f32,
                confidence: a.get("confidence").and_then(|c| c.as_f64()).unwrap_or(0.0) as f32,
            },
            "noul" => Answer::Noul {
                noul: a.get("noul").and_then(|n| n.as_f64()).unwrap_or(0.0) as f32,
            },
            other => bail!("未知的答案类型 `{other}`"),
        };
        out.insert(name.clone(), parsed);
    }
    Ok(out)
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        s.chars().take(n).collect::<String>() + "…"
    }
}

// ---------------------------------------------------------------------------
// WinHTTP
// ---------------------------------------------------------------------------

struct Url {
    secure: bool,
    host: String,
    port: u16,
    path: String,
}

fn parse_url(url: &str) -> Result<Url> {
    let (secure, rest) = if let Some(r) = url.strip_prefix("https://") {
        (true, r)
    } else if let Some(r) = url.strip_prefix("http://") {
        (false, r)
    } else {
        bail!("端点必须以 http:// 或 https:// 开头：{url}");
    };
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) => (h.to_string(), p.parse::<u16>().map_err(|_| anyhow!("端口非法：{p}"))?),
        None => (authority.to_string(), if secure { 443 } else { 80 }),
    };
    if host.is_empty() {
        bail!("端点缺少主机名：{url}");
    }
    Ok(Url { secure, host, port, path: path.to_string() })
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// 一次 POST，返回响应体文本。
fn http_post(
    url: &str,
    api_key: &str,
    body: &str,
    proxy: Option<&str>,
    timeout_ms: u32,
) -> Result<String> {
    let u = parse_url(url)?;
    let agent = wide("Anycast/0.1 (Windows; SystemOne)");
    let headers_text = format!(
        "Content-Type: application/json\r\nAuthorization: Bearer {api_key}\r\nAccept: application/json\r\n"
    );
    // ⚠️ headers 是按【长度】传的 slice，不是以 \0 结尾的 C 字符串。
    // 多带一个 \0 会让 WinHttpSendRequest 报 E_INVALIDARG(0x80070057)。
    let headers: Vec<u16> = headers_text.encode_utf16().collect();
    let verb = wide("POST");
    let path = wide(&u.path);
    let host = wide(&u.host);
    let proxy_w = proxy.map(wide);

    unsafe {
        let (access_type, proxy_ptr) = match &proxy_w {
            Some(p) => (WINHTTP_ACCESS_TYPE_NAMED_PROXY, PCWSTR(p.as_ptr())),
            None => (WINHTTP_ACCESS_TYPE_DEFAULT_PROXY, PCWSTR::null()),
        };

        let session = WinHttpOpen(PCWSTR(agent.as_ptr()), access_type, proxy_ptr, PCWSTR::null(), 0);
        if session.is_null() {
            bail!("WinHttpOpen 失败：{}", std::io::Error::last_os_error());
        }
        // 无论后面哪一步失败都要关句柄
        let result = (|| -> Result<String> {
            // 硬超时（§5.7）：接收阶段给满 timeout_ms，连接阶段略短
            WinHttpSetTimeouts(session, 1000, 1500, timeout_ms as i32, timeout_ms as i32)
                .map_err(|e| anyhow!("WinHttpSetTimeouts 失败：{e}"))?;

            let connect = WinHttpConnect(session, PCWSTR(host.as_ptr()), u.port, 0);
            if connect.is_null() {
                bail!("WinHttpConnect 失败（主机 {}）：{}", u.host, std::io::Error::last_os_error());
            }
            let result = (|| -> Result<String> {
                let flags = if u.secure {
                    WINHTTP_FLAG_SECURE
                } else {
                    WINHTTP_OPEN_REQUEST_FLAGS(0)
                };
                let req = WinHttpOpenRequest(
                    connect,
                    PCWSTR(verb.as_ptr()),
                    PCWSTR(path.as_ptr()),
                    PCWSTR::null(),
                    PCWSTR::null(),
                    std::ptr::null(),
                    flags,
                );
                if req.is_null() {
                    bail!("WinHttpOpenRequest 失败：{}", std::io::Error::last_os_error());
                }
                let result = (|| -> Result<String> {
                    let bytes = body.as_bytes();
                    WinHttpSendRequest(
                        req,
                        Some(headers.as_slice()),
                        Some(bytes.as_ptr() as *const core::ffi::c_void),
                        bytes.len() as u32,
                        bytes.len() as u32,
                        0,
                    )
                    .map_err(|e| anyhow!("WinHttpSendRequest 失败（超时 {timeout_ms} ms）：{e}"))?;

                    WinHttpReceiveResponse(req, std::ptr::null_mut())
                        .map_err(|e| anyhow!("WinHttpReceiveResponse 失败：{e}"))?;

                    let mut code: u32 = 0;
                    let mut len = std::mem::size_of::<u32>() as u32;
                    let _ = WinHttpQueryHeaders(
                        req,
                        WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
                        PCWSTR::null(),
                        Some(&mut code as *mut u32 as *mut core::ffi::c_void),
                        &mut len,
                        std::ptr::null_mut(),
                    );

                    let mut buf: Vec<u8> = Vec::new();
                    loop {
                        let mut avail: u32 = 0;
                        if WinHttpQueryDataAvailable(req, &mut avail).is_err() || avail == 0 {
                            break;
                        }
                        let mut chunk = vec![0u8; avail as usize];
                        let mut read: u32 = 0;
                        if WinHttpReadData(
                            req,
                            chunk.as_mut_ptr() as *mut core::ffi::c_void,
                            avail,
                            &mut read,
                        )
                        .is_err()
                            || read == 0
                        {
                            break;
                        }
                        chunk.truncate(read as usize);
                        buf.extend_from_slice(&chunk);
                    }

                    let text = String::from_utf8_lossy(&buf).to_string();
                    if !(200..300).contains(&code) {
                        bail!("Jev 返回 HTTP {code}：{}", truncate(&text, 300));
                    }
                    Ok(text)
                })();
                let _ = WinHttpCloseHandle(req);
                result
            })();
            let _ = WinHttpCloseHandle(connect);
            result
        })();
        let _ = WinHttpCloseHandle(session);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::decision::{slots, standard_questions};

    #[test]
    fn url_parsing() {
        let u = parse_url(OFFICIAL_ENDPOINT).unwrap();
        assert!(u.secure);
        assert_eq!(u.host, "api.typesafe.ai");
        assert_eq!(u.port, 443);
        assert_eq!(u.path, "/v1/systemone");

        let u = parse_url("http://127.0.0.1:7897/v1/decide").unwrap();
        assert!(!u.secure);
        assert_eq!(u.host, "127.0.0.1");
        assert_eq!(u.port, 7897);
        assert_eq!(u.path, "/v1/decide");

        // 无路径时补 /
        let u = parse_url("https://example.com").unwrap();
        assert_eq!(u.path, "/");

        assert!(parse_url("ftp://x").is_err());
        assert!(parse_url("https://").is_err());
    }

    #[test]
    fn body_matches_probe_shape() {
        // 请求体必须与 tools/jev_probe.py 同构，否则实测结论不适用
        let body = build_body("找一下昨天改的 docker 配置", &standard_questions(), "jev-latest");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();

        assert_eq!(v["model"], "jev-latest");
        assert_eq!(v["state"], "找一下昨天改的 docker 配置");

        let q = &v["questions"];
        assert_eq!(q["type"]["type"], "choice");
        assert_eq!(q["type"]["instructions"], slots::INSTRUCTIONS_TYPE);
        assert_eq!(q["type"]["criteria"]["code"], "源代码、脚本、配置、rs、py、js、json、yml、toml");
        assert_eq!(q["time"]["criteria"]["yesterday"], "昨天");
        assert_eq!(q["location"]["criteria"]["common"], "常用目录，如桌面、下载、文档、项目目录");

        // noul 必须【不带】criteria
        assert_eq!(q["is_search"]["type"], "noul");
        assert!(q["is_search"].get("criteria").is_none(), "noul 不该有 criteria");
        assert_eq!(q["is_natural"]["instructions"], slots::INSTRUCTIONS_IS_NATURAL);
    }

    #[test]
    fn calibrated_wording_is_preserved() {
        // 这条断言是「防手滑」：措辞改一个字，准确率可能从 96% 掉回 77%
        let body = build_body("x", &standard_questions(), "m");
        assert!(body.contains("用户是否在找本机的某个文件或文件夹？"), "is_search 措辞被改动了");
        assert!(body.contains("整句、名词短语都算"), "is_natural 措辞被改动了");
    }

    #[test]
    fn parse_full_response() {
        // 取自开发文档 §2.1.1 的官方响应样例
        let text = r#"{
          "model": "jev-1.13.0",
          "answers": {
            "type":      { "type": "choice", "choice": "code", "confidence": 0.96,
                           "probabilities": { "document": 0.04, "code": 0.96 } },
            "time":      { "type": "score", "score": 1.0, "confidence": 0.91,
                           "legend": { "0": "今天", "1": "昨天" }, "probabilities": { "1": 0.91 } },
            "is_search": { "type": "noul", "noul": 0.98 }
          },
          "usage": { "input_tokens": 434, "output_tokens": 75 }
        }"#;
        let a = parse_answers(text).unwrap();
        assert_eq!(a["type"].as_choice().unwrap().0, "code");
        assert!((a["type"].confidence() - 0.96).abs() < 1e-6);
        assert_eq!(a["is_search"].as_noul().unwrap(), 0.98);
        assert!(matches!(a["time"], Answer::Score { .. }));
    }

    #[test]
    fn parse_rejects_garbage() {
        assert!(parse_answers("not json").is_err());
        assert!(parse_answers(r#"{"model":"x"}"#).is_err(), "缺 answers 应报错");
    }

    #[test]
    fn unavailable_without_key() {
        let b = JevBackend::new(OFFICIAL_ENDPOINT, "  ", DEFAULT_MODEL);
        assert!(!b.is_available());
        assert!(b.decide("x", &standard_questions()).is_err());
    }

    #[test]
    fn env_name_depends_on_endpoint() {
        assert_eq!(JevBackend::env_name_for(OFFICIAL_ENDPOINT), ENV_OFFICIAL);
        assert_eq!(JevBackend::env_name_for(HOSTED_ENDPOINT), ENV_HOSTED);
    }

    /// **桩服务端到端**：验证真正发出去的字节，而不只是 `build_body` 的返回值。
    ///
    /// 这条路径此前只有 `build_body` / `parse_answers` 的单元测试 ——
    /// 传输层本身（WinHTTP 的请求行、头、读体）**一次都没跑过**，
    /// 而 `WinHttpSendRequest` 的 headers 长度语义就踩过一次坑
    /// （多带一个 `\0` → `E_INVALIDARG` 0x80070057）。
    /// 那段代码只有真发一次请求才能覆盖。
    ///
    /// 用桩服务而不是真实端点：不烧钱、不需要 Key、不依赖网络。
    /// 本机若把 Clash 设成了系统代理，WinHTTP 默认会绕过 `<local>`，
    /// 所以 `127.0.0.1` 不受影响；万一这里开始失败，先查代理的绕过列表。
    #[test]
    fn http_roundtrip_against_a_stub_server() {
        use crate::core::decision::{Intent, TypeSlot};
        use std::io::{BufRead, BufReader, Read, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").expect("绑定本地端口");
        let port = listener.local_addr().unwrap().port();

        // 响应体形状与官方一致（§2.1.1）：answers 里每项带 type
        let payload = r#"{"answers":{"type":{"type":"choice","choice":"code","confidence":0.91},"is_search":{"type":"noul","noul":0.93}}}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            payload.len(),
            payload
        );

        let server = std::thread::spawn(move || {
            let (mut sock, _) = listener.accept().expect("应当收到一次请求");
            let mut reader = BufReader::new(sock.try_clone().expect("复制句柄"));
            let mut head = String::new();
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 {
                    break;
                }
                let end_of_head = line == "\r\n" || line == "\n";
                head.push_str(&line);
                if end_of_head {
                    break;
                }
            }
            let len: usize = head
                .lines()
                .find_map(|l| {
                    let (k, v) = l.split_once(':')?;
                    if k.eq_ignore_ascii_case("content-length") {
                        v.trim().parse().ok()
                    } else {
                        None
                    }
                })
                .unwrap_or(0);
            let mut body = vec![0u8; len];
            let _ = reader.read_exact(&mut body);
            let _ = sock.write_all(response.as_bytes());
            let _ = sock.flush();
            (head, String::from_utf8_lossy(&body).to_string())
        });

        let backend = JevBackend::new(
            format!("http://127.0.0.1:{port}/v1/systemone"),
            "test-key",
            "jev-latest",
        )
        .with_timeout(5_000);
        let answers = backend
            .decide("找一下 docker 配置", &standard_questions())
            .expect("桩服务应当返回可解析的响应");

        let (head, sent) = server.join().expect("桩线程不应 panic");

        // ── 请求侧：这几条以前没有任何测试覆盖 ──
        assert!(
            head.starts_with("POST /v1/systemone "),
            "请求行不对（路径或方法错了）：{head}"
        );
        assert!(
            head.to_lowercase().contains("authorization: bearer test-key"),
            "缺 Authorization 头：{head}"
        );
        assert!(
            head.to_lowercase().contains("content-type: application/json"),
            "缺 Content-Type 头：{head}"
        );

        let sent_json: serde_json::Value = serde_json::from_str(&sent).expect("请求体应当是合法 JSON");
        assert_eq!(sent_json["state"], "找一下 docker 配置");
        assert_eq!(sent_json["model"], "jev-latest");
        assert!(
            sent_json["questions"]["type"]["criteria"].is_object(),
            "choice 的 criteria 必须是**映射**（§2.1.1：score 是有序数组、noul 不传）"
        );
        assert_eq!(sent_json["questions"]["type"]["type"], "choice");

        // ── 响应侧：解析回来的答案要能组装成 Intent ──
        let it = Intent::from_answers(&answers, backend.id());
        assert_eq!(it.type_slot, TypeSlot::Code);
        assert!(it.is_search);
        assert_eq!(it.backend, "jev");
    }
}
