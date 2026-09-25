# Jev 探针实测报告

- 端点：`https://api.typesafe.ai/v1/systemone`
- 模型：`jev-latest`
- 代理：http://127.0.0.1:7897
- 用例：成功 12 / 失败 0
- 延迟：min 747 ms / p50 997 ms / max 15138 ms / 平均 2530 ms

## 槽位准确率

| 分组 | 判对 | 总数 | 准确率 |
| --- | --- | --- | --- |
| zh | 8 | 10 | 80% |
| en | 5 | 7 | 71% |
| mix | 3 | 3 | 100% |
| kw | 2 | 4 | 50% |
| neg | 2 | 2 | 100% |
| **合计** | **20** | **26** | **77%** |

## 逐用例明细

### `zh1` 找一下昨天改的 docker 配置

- 延迟 785 ms，tokens in=872 out=231
- 输出：is_search=0.60，is_natural=0.95，type=`code`(1.00)，time=`yesterday`(1.00)，location=`any`(0.98)

| 槽位 | 期望 | 实际 | 结果 |
| --- | --- | --- | --- |
| is_search | True | True | ✅ |
| type | code | code | ✅ |
| time | yesterday | yesterday | ✅ |

### `zh2` 那个写存储逻辑的文档

- 延迟 15138 ms，tokens in=870 out=229
- 输出：is_search=0.30，is_natural=0.60，type=`document`(1.00)，time=`any`(1.00)，location=`any`(0.99)

| 槽位 | 期望 | 实际 | 结果 |
| --- | --- | --- | --- |
| is_search | True | False | ❌ |
| type | document | document | ✅ |

### `zh3` 我的项目文件夹在哪

- 延迟 747 ms，tokens in=869 out=229
- 输出：is_search=0.29，is_natural=0.97，type=`folder`(1.00)，time=`any`(1.00)，location=`any`(0.83)

| 槽位 | 期望 | 实际 | 结果 |
| --- | --- | --- | --- |
| is_search | True | False | ❌ |
| type | folder | folder | ✅ |

### `zh4` 上周下载的那个压缩包

- 延迟 1191 ms，tokens in=870 out=230
- 输出：is_search=0.66，is_natural=0.60，type=`archive`(1.00)，time=`last_week`(1.00)，location=`common`(0.95)

| 槽位 | 期望 | 实际 | 结果 |
| --- | --- | --- | --- |
| is_search | True | True | ✅ |
| type | archive | archive | ✅ |
| time | last_week | last_week | ✅ |

### `en1` files I modified yesterday

- 延迟 971 ms，tokens in=864 out=231
- 输出：is_search=0.44，is_natural=0.46，type=`all`(0.95)，time=`yesterday`(1.00)，location=`any`(0.99)

| 槽位 | 期望 | 实际 | 结果 |
| --- | --- | --- | --- |
| is_search | True | False | ❌ |
| time | yesterday | yesterday | ✅ |

### `en2` the doc about storage logic

- 延迟 755 ms，tokens in=865 out=229
- 输出：is_search=0.33，is_natural=0.42，type=`document`(0.99)，time=`any`(1.00)，location=`any`(0.95)

| 槽位 | 期望 | 实际 | 结果 |
| --- | --- | --- | --- |
| is_search | True | False | ❌ |
| type | document | document | ✅ |

### `en3` any png screenshots from last week

- 延迟 937 ms，tokens in=866 out=230
- 输出：is_search=0.74，is_natural=0.49，type=`image`(1.00)，time=`last_week`(1.00)，location=`any`(0.99)

| 槽位 | 期望 | 实际 | 结果 |
| --- | --- | --- | --- |
| is_search | True | True | ✅ |
| type | image | image | ✅ |
| time | last_week | last_week | ✅ |

### `mix1` 找 Dockerfile 昨天改的

- 延迟 2277 ms，tokens in=870 out=231
- 输出：is_search=0.68，is_natural=0.69，type=`code`(0.98)，time=`yesterday`(1.00)，location=`any`(0.90)

| 槽位 | 期望 | 实际 | 结果 |
| --- | --- | --- | --- |
| is_search | True | True | ✅ |
| type | code | code | ✅ |
| time | yesterday | yesterday | ✅ |

### `kw1` docker

- 延迟 2788 ms，tokens in=861 out=229
- 输出：is_search=0.12，is_natural=0.05，type=`all`(0.68)，time=`any`(1.00)，location=`any`(1.00)

| 槽位 | 期望 | 实际 | 结果 |
| --- | --- | --- | --- |
| is_search | True | False | ❌ |
| is_natural | False | False | ✅ |

### `kw2` storage.rs

- 延迟 813 ms，tokens in=862 out=229
- 输出：is_search=0.46，is_natural=0.04，type=`code`(1.00)，time=`any`(1.00)，location=`any`(0.70)

| 槽位 | 期望 | 实际 | 结果 |
| --- | --- | --- | --- |
| is_search | True | False | ❌ |
| is_natural | False | False | ✅ |

### `neg1` 今天天气怎么样

- 延迟 2965 ms，tokens in=867 out=229
- 输出：is_search=0.01，is_natural=0.98，type=`all`(1.00)，time=`today`(0.98)，location=`any`(1.00)

| 槽位 | 期望 | 实际 | 结果 |
| --- | --- | --- | --- |
| is_search | False | False | ✅ |

### `neg2` 帮我写一段快排

- 延迟 997 ms，tokens in=867 out=229
- 输出：is_search=0.02，is_natural=0.86，type=`code`(1.00)，time=`any`(1.00)，location=`any`(1.00)

| 槽位 | 期望 | 实际 | 结果 |
| --- | --- | --- | --- |
| is_search | False | False | ✅ |

