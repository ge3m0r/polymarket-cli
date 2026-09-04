# Polymarket CLI 中文使用指南

本文面向本地使用、脚本调用和交易操作。项目当前版本为 `0.1.4`，可执行文件名为 `polymarket`。

> 注意：这是实验性交易工具。涉及私钥、授权、下单和链上交易时，请先用小额资金验证；不要把真实私钥写入命令历史、脚本或版本库。

## 1. 编译与运行

要求 Rust `1.88.0` 或更高版本。

```bash
# 在项目根目录编译优化版本
cargo build --release --locked

# 查看版本和帮助
./target/release/polymarket --version
./target/release/polymarket --help
```

编译产物位于：

```text
target/release/polymarket
```

如需安装到 Cargo 的可执行目录：

```bash
cargo install --path . --locked
polymarket --version
```

下文统一使用已加入 `PATH` 的 `polymarket`。如果没有安装，请将其替换为 `./target/release/polymarket`。

## 2. 基本语法

```text
polymarket [全局参数] <命令组> <子命令> [参数]
```

全局参数：

| 参数 | 含义 |
| --- | --- |
| `-o, --output table\|json` | 输出格式，默认 `table` |
| `--private-key <KEY>` | 本次调用使用的私钥，优先级最高 |
| `--signature-type <TYPE>` | `proxy`、`eoa` 或 `gnosis-safe` |
| `-h, --help` | 查看帮助 |
| `-V, --version` | 查看版本 |

随时可用分层帮助确认参数：

```bash
polymarket --help
polymarket markets --help
polymarket markets list --help
polymarket clob create-order --help
```

## 3. 无钱包快速开始

市场浏览、公开资料、订单簿和公开链上数据不需要钱包：

```bash
# API 健康检查
polymarket status
polymarket clob ok

# 浏览和搜索市场
polymarket markets list --active true --limit 5
polymarket markets search "bitcoin" --limit 5
polymarket markets get <市场ID或slug>

# 浏览事件
polymarket events list --tag crypto --active true --limit 10
polymarket events get <事件ID或slug>

# 查询订单簿和价格；这里需要 token ID，而不是 market ID
polymarket clob book <TOKEN_ID>
polymarket clob midpoint <TOKEN_ID>
polymarket clob spread <TOKEN_ID>
polymarket clob price-history <TOKEN_ID> --interval 1d --fidelity 30

# 查询公开地址的仓位
polymarket data positions <0x钱包地址>
polymarket data value <0x钱包地址>
```

## 4. JSON 与脚本调用

使用 `-o json` 获取机器可读输出：

```bash
polymarket -o json markets list --limit 100
polymarket -o json clob midpoint <TOKEN_ID>
polymarket -o json data positions <0x钱包地址>
```

配合 `jq`：

```bash
polymarket -o json markets list --limit 100 | jq '.[].question'
polymarket -o json clob midpoint <TOKEN_ID> | jq '.mid'
```

错误约定：

- `table` 模式把 `Error: ...` 写到标准错误。
- `json` 模式把 `{"error":"..."}` 写到标准输出。
- 两种模式失败时都会返回非零退出码。

脚本中应同时检查退出码和输出：

```bash
if result=$(polymarket -o json markets list --limit 5); then
  printf '%s\n' "$result" | jq .
else
  echo "Polymarket 请求失败" >&2
  exit 1
fi
```

## 5. 钱包与认证

### 推荐：交互式初始化

```bash
polymarket setup
```

也可以分步配置：

```bash
# 创建新钱包
polymarket wallet create

# 或导入已有私钥
polymarket wallet import <PRIVATE_KEY>

# 查看配置和地址，不会显示私钥
polymarket wallet show
polymarket wallet address
```

默认配置文件：

```text
~/.config/polymarket/config.json
```

私钥的读取优先级为：

1. `--private-key`
2. `POLYMARKET_PRIVATE_KEY` 环境变量
3. 配置文件

签名类型的读取优先级同样是命令参数、环境变量、配置文件，默认值为 `proxy`：

```bash
export POLYMARKET_PRIVATE_KEY='<PRIVATE_KEY>'
export POLYMARKET_SIGNATURE_TYPE='proxy'
```

可覆盖服务地址：

```bash
export POLYMARKET_CLOB_HOST='https://clob.polymarket.com'
export POLYMARKET_RPC_URL='https://polygon.drpc.org'
```

安全建议：

- 优先使用配置文件或临时环境变量，避免 `--private-key` 留在 shell 历史中。
- 不要提交 `config.json`、私钥、助记词或 API 凭证。
- `wallet reset` 会删除本地配置和密钥；确认已备份后再执行。

## 6. 命令总览

| 命令组 | 用途 | 常用子命令 |
| --- | --- | --- |
| `markets` | 市场浏览 | `list`、`get`、`search`、`tags` |
| `events` | 事件浏览 | `list`、`get`、`tags` |
| `tags` | 标签查询 | `list`、`get`、`related`、`related-tags` |
| `series` | 系列事件 | `list`、`get` |
| `comments` | 评论查询 | `list`、`get`、`by-user` |
| `profiles` | 公开资料 | `get` |
| `sports` | 体育元数据 | `list`、`market-types`、`teams` |
| `clob` | 价格、订单簿、交易和账户 | 见下一节 |
| `data` | 仓位、成交、活动、排行榜 | `positions`、`trades`、`leaderboard` 等 |
| `approve` | 检查或设置合约授权 | `check`、`set` |
| `ctf` | 拆分、合并、赎回条件代币 | `split`、`merge`、`redeem` 等 |
| `bridge` | 跨链充值信息 | `deposit`、`supported-assets`、`status` |
| `weather` | 天气市场研究（只读） | `tokyo` |
| `wallet` | 本地钱包管理 | `create`、`import`、`address`、`show`、`reset` |
| `setup` | 首次配置向导 | 无子命令 |
| `shell` | 交互式终端 | 无子命令 |
| `status` | Gamma API 健康检查 | 无子命令 |
| `upgrade` | 更新已安装版本 | 无子命令 |

## 7. CLOB：行情与订单簿

以下命令不需要钱包：

```bash
# 单个 token
polymarket clob price <TOKEN_ID> --side buy
polymarket clob midpoint <TOKEN_ID>
polymarket clob spread <TOKEN_ID>
polymarket clob book <TOKEN_ID>
polymarket clob last-trade <TOKEN_ID>

# 多个 token，使用英文逗号分隔
polymarket clob batch-prices "<TOKEN_1>,<TOKEN_2>" --side sell
polymarket clob midpoints "<TOKEN_1>,<TOKEN_2>"
polymarket clob books "<TOKEN_1>,<TOKEN_2>"

# 市场和元数据
polymarket clob market <0xCONDITION_ID>
polymarket clob markets
polymarket clob tick-size <TOKEN_ID>
polymarket clob fee-rate <TOKEN_ID>
polymarket clob neg-risk <TOKEN_ID>
polymarket clob geoblock
```

价格历史的 `--interval` 可选 `1m`、`1h`、`6h`、`1d`、`1w`、`max`。

## 8. 下单、查询与撤单

这些命令需要钱包认证。交易前先检查资金和授权：

```bash
polymarket wallet show
polymarket approve check
polymarket clob balance --asset-type collateral
```

首次交易通常需要链上授权；`approve set` 会发送交易并消耗 Polygon gas：

```bash
polymarket approve set
```

限价单：

```bash
polymarket clob create-order \
  --token <TOKEN_ID> \
  --side buy \
  --price 0.50 \
  --size 10
```

市价单：

```bash
# 买入时 amount 表示 pUSD 金额；卖出时表示份额数量
polymarket clob market-order \
  --token <TOKEN_ID> \
  --side buy \
  --amount 5
```

订单类型：

- `GTC`：持续有效，限价单默认值。
- `FOK`：全部立即成交，否则取消，市价单默认值。
- `FAK`：立即成交可成交部分，其余取消。
- `GTD`：有效至指定时间；当前 CLI 暴露该类型，但下单参数中没有单独的到期时间字段，使用前应先确认服务端行为。

查询和撤单：

```bash
polymarket clob orders
polymarket clob orders --market <0xCONDITION_ID>
polymarket clob order <ORDER_ID>
polymarket clob trades

polymarket clob cancel <ORDER_ID>
polymarket clob cancel-orders "<ORDER_1>,<ORDER_2>"
polymarket clob cancel-market --market <0xCONDITION_ID>
polymarket clob cancel-all
```

`cancel-all`、`approve set`、`ctf split/merge/redeem` 等操作会改变账户或链上状态，执行前务必核对钱包、网络、市场和数量。

## 9. 公开数据查询

```bash
# 钱包维度
polymarket data positions <0x钱包地址> --limit 25
polymarket data closed-positions <0x钱包地址>
polymarket data value <0x钱包地址>
polymarket data traded <0x钱包地址>
polymarket data trades <0x钱包地址> --limit 50
polymarket data activity <0x钱包地址>

# 市场和事件维度
polymarket data holders <0xCONDITION_ID> --limit 10
polymarket data open-interest <0xCONDITION_ID>
polymarket data volume <事件ID>

# 排行榜
polymarket data leaderboard --period month --order-by pnl --limit 10
polymarket data builder-leaderboard --period week
polymarket data builder-volume --period month
```

`--period` 可选 `day`、`week`、`month`、`all`；`--order-by` 可选 `pnl`、`vol`。

## 10. CTF 与跨链充值

CTF 的链上写操作需要钱包和 Polygon gas：

```bash
polymarket ctf split --condition <0xCONDITION_ID> --amount 10
polymarket ctf merge --condition <0xCONDITION_ID> --amount 10
polymarket ctf redeem --condition <0xCONDITION_ID>
polymarket ctf redeem-neg-risk --condition <0xCONDITION_ID> --amounts "10,5"
```

只读 ID 计算：

```bash
polymarket ctf condition-id --oracle <0xORACLE> --question <0xQUESTION_ID> --outcomes 2
polymarket ctf collection-id --condition <0xCONDITION_ID> --index-set 1
polymarket ctf position-id --collection <0xCOLLECTION_ID>
```

跨链充值：

```bash
polymarket bridge supported-assets
polymarket bridge deposit <0xPOLYMARKET钱包地址>
polymarket bridge status <充值地址>
```

## 11. 常见标识符

| 名称 | 典型格式 | 主要用途 |
| --- | --- | --- |
| 市场 ID | 数字字符串 | `markets get/tags` |
| 市场 slug | 可读短名 | `markets get` |
| 事件 ID | 数字 | `events get`、`data volume` |
| Token ID / Asset ID | 很长的十进制数字 | `clob price/book/order`、条件代币余额 |
| Condition ID | `0x` 开头的 32 字节十六进制值 | CLOB 市场筛选、CTF、持仓统计 |
| 钱包地址 | `0x` 开头的 20 字节地址 | 钱包、仓位、充值查询 |
| Order ID | CLOB 返回的订单标识 | 订单查询和撤单 |

不要混用 market ID、token ID 和 condition ID。拿不准时先使用 JSON 输出查看完整市场对象：

```bash
polymarket -o json markets get <市场ID或slug> | jq .
```

## 12. 交互模式与排错

### 东京天气研究原型

```bash
# 默认分析东京时间的明天
polymarket weather tokyo

# 指定日期、集合模型和站点偏差修正
polymarket weather tokyo --date 2026-09-05 --model gfs_seamless --bias-c 0.0

# 获取完整 JSON（含 token ID、数据源和对冲扫描结果）
polymarket -o json weather tokyo --date 2026-09-05

# 生成最终75/25策略的只读模拟信号；保守期望利润不足1 USDC时返回 SKIP
polymarket -o json weather signal-tokyo \
  --legacy-weight 0.75 --size 5 --slippage 0.01 --min-expected-pnl 1

# 默认回测最近 14 个连续的已结算东京事件
polymarket weather backtest-tokyo

# 改为最近 30 个事件
polymarket weather backtest-tokyo --recent 30

# 中和策略：50% 旧版“下二上一”，50% 完整分布优化器
polymarket weather backtest-tokyo --legacy-weight 0.5

# 每档 5 份、前一天 05:00 UTC 建仓，并按每份 0.01 USDC 额外滑点计算保守场景
polymarket weather backtest-tokyo --since 2026-04-01 --until 2026-08-31 \
  --lead-days 1 --size 5 --entry-hour-utc 5 --slippage 0.01
```

这些命令只读取公开数据，不读取钱包，也不会下单。回测会用前一日归档的 GEFS 均值/离散度重建 31 条成员轨迹，对每条轨迹取日最高温，枚举全部连续四档，并选择扣除手续费和滑点后期望盈亏最高的组合；最高值不为正时空仓。`--legacy-weight` 接受 0–1：`0.5` 代表一半仓位使用旧版偏低四档、一半使用分布优化四档，再对混合组合应用同一个交易门槛。输出包含逐日概率覆盖、决策、成本、兑付、实际盈亏和累计 ROI。

仓库中的 `.github/workflows/tokyo-weather-paper.yml` 默认每天 UTC 05:05（东京14:05）在 GitHub 云端运行 `signal-tokyo`，把 JSON 保存为90天 artifact，并在 Actions 运行摘要显示 BUY/SKIP、两个温度区间、成本和保守期望盈亏。云端任务不依赖本机开机。

历史逐成员预报只保留约三天，因此 31 条轨迹是从逐小时 ensemble mean/spread 重建的近似分布，并非原始成员精确回放。历史 CLOB 价格也不是完整订单簿快照，保守结果用 `--slippage` 补偿部分执行偏差。详细方法、数据源和风险见
[东京天气研究原型](docs/TOKYO_WEATHER_RESEARCH.zh-CN.md)。

交互模式中不需要重复输入 `polymarket` 前缀：

```text
$ polymarket shell
polymarket> markets list --limit 3
polymarket> clob book <TOKEN_ID>
polymarket> exit
```

常见排错顺序：

```bash
polymarket --version
polymarket status
polymarket clob ok
polymarket wallet show
polymarket clob geoblock
```

- 提示 `No wallet configured`：运行 `wallet create`、`wallet import`，或配置 `POLYMARKET_PRIVATE_KEY`。
- CLOB 认证失败：检查私钥、`--signature-type`，以及代理钱包类型是否匹配。
- Polygon RPC 连接失败：检查网络，或设置可用的 `POLYMARKET_RPC_URL`。
- 参数不确定：对具体子命令追加 `--help`，以当前二进制输出为准。
- `cargo build --locked` 无法下载依赖：检查 Cargo registry/镜像配置和网络连通性，再重试。
