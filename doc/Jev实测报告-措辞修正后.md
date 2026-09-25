# Jev 探针实测报告

- 端点：`https://api.typesafe.ai/v1/systemone`
- 模型：`jev-latest`
- 代理：http://127.0.0.1:7897
- 用例：成功 12 / 失败 0
- 延迟：min 775 ms / p50 941 ms / max 2455 ms / 平均 1204 ms

## 槽位准确率

| 分组 | 判对 | 总数 | 准确率 |
| --- | --- | --- | --- |
| zh | 10 | 10 | 100% |
| en | 7 | 7 | 100% |
| mix | 3 | 3 | 100% |
| kw | 3 | 4 | 75% |
| neg | 2 | 2 | 100% |
| **合计** | **25** | **26** | **96%** |

## 逐用例明细

### `zh1` 找一下昨天改的 docker 配置

- 延迟 973 ms，tokens in=934 out=231
- 输出：is_search=0.93，is_natural=0.97，type=`code`(1.00)，time=`yesterday`(1.00)，location=`any`(0.98)

| 槽位 | 期望 | 实际 | 结果 |
| --- | --- | --- | --- |
| is_search | True | True | ✅ |
| type | code | code | ✅ |
| time | yesterday | yesterday | ✅ |

### `zh2` 那个写存储逻辑的文档

- 延迟 780 ms，tokens in=932 out=229
- 输出：is_search=0.60，is_natural=0.93，type=`document`(1.00)，time=`any`(1.00)，location=`any`(0.99)

| 槽位 | 期望 | 实际 | 结果 |
| --- | --- | --- | --- |
| is_search | True | True | ✅ |
| type | document | document | ✅ |

### `zh3` 我的项目文件夹在哪

- 延迟 805 ms，tokens in=931 out=229
- 输出：is_search=0.93，is_natural=0.95，type=`folder`(1.00)，time=`any`(1.00)，location=`any`(0.86)

| 槽位 | 期望 | 实际 | 结果 |
| --- | --- | --- | --- |
| is_search | True | True | ✅ |
| type | folder | folder | ✅ |

### `zh4` 上周下载的那个压缩包

- 延迟 941 ms，tokens in=932 out=230
- 输出：is_search=0.94，is_natural=0.93，type=`archive`(1.00)，time=`last_week`(1.00)，location=`common`(0.92)

| 槽位 | 期望 | 实际 | 结果 |
| --- | --- | --- | --- |
| is_search | True | True | ✅ |
| type | archive | archive | ✅ |
| time | last_week | last_week | ✅ |

### `en1` files I modified yesterday

- 延迟 2455 ms，tokens in=926 out=231
- 输出：is_search=0.85，is_natural=0.95，type=`all`(0.94)，time=`yesterday`(1.00)，location=`any`(0.99)

| 槽位 | 期望 | 实际 | 结果 |
| --- | --- | --- | --- |
| is_search | True | True | ✅ |
| time | yesterday | yesterday | ✅ |

### `en2` the doc about storage logic

- 延迟 775 ms，tokens in=927 out=229
- 输出：is_search=0.59，is_natural=0.92，type=`document`(1.00)，time=`any`(1.00)，location=`any`(0.95)

| 槽位 | 期望 | 实际 | 结果 |
| --- | --- | --- | --- |
| is_search | True | True | ✅ |
| type | document | document | ✅ |

### `en3` any png screenshots from last week

- 延迟 790 ms，tokens in=928 out=230
- 输出：is_search=0.93，is_natural=0.96，type=`image`(1.00)，time=`last_week`(1.00)，location=`any`(0.99)

| 槽位 | 期望 | 实际 | 结果 |
| --- | --- | --- | --- |
| is_search | True | True | ✅ |
| type | image | image | ✅ |
| time | last_week | last_week | ✅ |

### `mix1` 找 Dockerfile 昨天改的

- 延迟 919 ms，tokens in=932 out=231
- 输出：is_search=0.95，is_natural=0.94，type=`code`(0.99)，time=`yesterday`(1.00)，location=`any`(0.89)

| 槽位 | 期望 | 实际 | 结果 |
| --- | --- | --- | --- |
| is_search | True | True | ✅ |
| type | code | code | ✅ |
| time | yesterday | yesterday | ✅ |

### `kw1` docker

- 延迟 2332 ms，tokens in=923 out=229
- 输出：is_search=0.38，is_natural=0.15，type=`all`(0.70)，time=`any`(1.00)，location=`any`(1.00)

| 槽位 | 期望 | 实际 | 结果 |
| --- | --- | --- | --- |
| is_search | True | False | ❌ |
| is_natural | False | False | ✅ |

### `kw2` storage.rs

- 延迟 921 ms，tokens in=924 out=229
- 输出：is_search=0.74，is_natural=0.09，type=`code`(1.00)，time=`any`(1.00)，location=`any`(0.70)

| 槽位 | 期望 | 实际 | 结果 |
| --- | --- | --- | --- |
| is_search | True | True | ✅ |
| is_natural | False | False | ✅ |

### `neg1` 今天天气怎么样

- 延迟 985 ms，tokens in=929 out=229
- 输出：is_search=0.01，is_natural=0.46，type=`all`(0.99)，time=`today`(0.98)，location=`any`(0.99)

| 槽位 | 期望 | 实际 | 结果 |
| --- | --- | --- | --- |
| is_search | False | False | ✅ |

### `neg2` 帮我写一段快排

- 延迟 1778 ms，tokens in=929 out=229
- 输出：is_search=0.01，is_natural=0.93，type=`code`(1.00)，time=`any`(1.00)，location=`any`(1.00)

| 槽位 | 期望 | 实际 | 结果 |
| --- | --- | --- | --- |
| is_search | False | False | ✅ |

