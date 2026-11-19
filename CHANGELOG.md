# UPDATE

## 2026-11-19 — 客户模型折扣支持「预约生效」

- 口径：某模型**首次**配折扣仍在用户「模型折扣」页直接填、当场生效；**已有折扣的模型要改值只能提交预约**。预约只有「指定日期、站点时区次日 0 点生效」一种，没有「立即生效」分支
- 目的：让一个自然日内同一客户同一模型只有一个折扣，日消耗与成本统计不再出现「当天两段」。由于即时保存不再改写已有键，**全库唯一会写折扣值的代码就是 0 点应用器**
- 当天就要改或改错了：把该模型从「已选模型折扣」里删掉再重新设置（删键与新增键不受限，这是有意保留的唯一后门）
- 服务端硬拦：`PUT /api/v1/users/{id}` 改到已有 MID 的值直接 400 并列出 MID，绕过前端直调接口同样拦得住
- 新表 `user_model_discount_schedules`（`pending / applied / cancelled` 三态即变更台账，不再单独做日志表）；同用户 + 同模型 + 同生效日的 pending 唯一（部分唯一索引）
- 应用器 `run_discount_schedule_loop`：启动先补跑一次（覆盖 0 点发布 / 重启窗口），之后睡到站点时区次日 00:00；0 点这一小时内若失败则每 5 分钟重试。多节点并发靠 `FOR UPDATE SKIP LOCKED` + `users` 行锁 + `status = 'pending'` 判据保证幂等，不引咨询锁
- 写回只覆盖预约涉及的 MID，其余键逐字节保留；用户已不存在则跳过其预约并告警
- 新接口：`GET / POST /api/v1/users/{id}/discount-schedules`、`DELETE /api/v1/discount-schedules/{id}`（取消只置 `cancelled` 留痕，不物理删）
- 台账 `created_at` 由代码显式写站点时区串：该列是 TEXT，列默认值 `now()::text` 会跟 PG 会话时区走（测试机 UTC、生产 Asia/Shanghai），不显式写就会出现两种时间口径
- 部署需重启后端以执行迁移 `user_model_discount_schedules_v1`

### 涉及文件
- `backend/src/api/user_discount_schedules.rs`（新增）、`backend/src/api/mod.rs`、`backend/src/api/users.rs`、`backend/src/db/migrations.rs`、`backend/src/main.rs`
- `frontend/src/pages/Users/Users.tsx`

---

## 2026-11-04 — 高可用组内可单独配置上游优先级 / 权重

- 优先级 / 权重从「上游渠道配置预设上的全局字段」变成**绑定级**属性：同一条上游可在 A 组当主用、在 B 组当备用，不必再按组复制上游记录
- 存储为 `channels.config` 的平级新键 `sub_channel_policy`（`{ "<channel_config_id>": { "priority": int, "weight": int } }`）；`sub_channels` 仍是纯 id 数组 —— 无 DDL、无迁移、无新表，回滚后旧代码照常读
- 生效值口径：`组内值 ?? 上游全局值`。整组在**实际改动任一行组内 P/W** 时才物化：先把全部已绑上游的当前生效值写满，再覆盖被改的那一行，避免组内/全局两套刻度混比
- 选渠只把 `select_channel` 子渠档的 3 处取值换成生效值（最高档判定 + 同档过滤 + 同档加权随机）；两级调度结构与计费口径不变
- 解析逐条目容错：单个坏值只丢该条，畸形 JSON 整组回落全局值，请求不因此失败
- 前端：已绑上游行内「组内优先级 0–999 / 组内权重 0–100」整数输入（留空 = 跟随该上游全局值）、「生效顺序」预览（主用 / 备用 / 同档分流比例 + 熔断 / 禁用状态）、主用档权重合计为 0 与单条权重为 0 的告警、HA 语境 2 处只读标签改为「生效值（组内 / 全局来源）」
- 上游渠道配置预设页加一行只读提示「优先级 / 权重请在分组内设置」；该页全局 P/W 表单项、初值与保存逻辑均未改动，仍是未物化组的回落来源
- 存量未配置组行为与升级前逐字等价，属向后兼容的增量能力

### 涉及文件
- `backend/src/relay/ha.rs`（新增 `SubChannelPolicy` / `parse_sub_channel_policy`）、`backend/src/relay/router.rs`
- `frontend/src/pages/Channels/Channels.tsx`、`frontend/src/pages/Channels/ChannelConfigs.tsx`

---

## 2026-08-02 — 控制台顶栏毛玻璃（用户端/管理端）

- `DashboardLayout` 顶栏改为半透明 + `backdrop-filter`；内容区铺满顶栏下方，滚动时穿过毛玻璃（用户端与管理端共用布局一并生效）

---

## 2026-08-02 — API 教程顶栏毛玻璃

- `/docs`（RelayAPI）顶部导航与模型广场一致：半透明 + `backdrop-filter` 模糊
- 修正布局：文档滚动区铺满顶栏下方，滚动时正文穿过顶栏，毛玻璃才可见（此前 `paddingTop` 在外层导致只能糊纯色底）

---

## 2026-08-02 — 用户端模型广场顶栏毛玻璃

- `/home/models` 顶部导航改为半透明 + `backdrop-filter` 模糊，内容滚动时可透过顶栏看到毛玻璃效果

---

## 2026-08-02 — 上游渠道配置启用/禁用状态

- `channel_configs` 新增 `status`（1=启用 / 0=禁用），管理端列表支持全部/激活/已禁用筛选与操作栏一键开关，编辑弹窗可改状态
- 模型渠道分组选择上游时，禁用配置置灰不可新选；选渠路由与 HA 子配过滤会跳过禁用上游，绑定后也无法实际使用
- 重启后端以执行迁移 `channel_configs_status_v1`

### 涉及文件
- `backend/src/db/migrations.rs`、`models/channel_config.rs`、`api/channel_configs.rs`、`relay/router.rs`
- `frontend/src/pages/Channels/ChannelConfigs.tsx`、`Channels.tsx`、`types/index.ts`

## 2026-07-31 — 系统概览：模型分布明细固定近 3 天

- 排行/进度条仍按当前筛选区间（如「今日」）；标签明细锚定区间末日向前固定 3 个自然日，不再被短区间截成一天
- 历史日一次读 `usage_daily_stats` 归档，仅当日走 realtime，避免按日循环放大查询

### 涉及文件
- `backend/src/api/date_helper.rs`、`dashboard.rs`
- `backend/src/relay/usage_stats.rs`
- `frontend/src/pages/Dashboard/Dashboard.tsx`

## 2026-07-31 — 出站 HTTP 客户端连接层加固

- 新增 `services/http_client`：建连超时、TCP keepalive、空闲连接回收；共享 Client 不设总超时（流式安全）
- 非流式上游请求统一挂 1800s 防挂死超时（可用环境变量覆盖）；火山监控 Client 复用同一连接基线
- README 补充可选 `HTTP_*` 调优环境变量

## 2026-07-31 — 数据同步：多站点请求密钥

- 本站密钥改为多密钥：可命名/备注、设置有效期、配置 IP 白名单（留空不限制）
- 导出鉴权匹配任一启用且未过期密钥，并校验 IP；旧单密钥自动迁移到 `data_sync_keys`

## 2026-07-31 — 站点插件「数据同步」

- 新增系统内置插件 `data_sync`：源站生成站点请求密钥，下游配置 URL + 密钥后可拉取模型目录与计费规则
- 冲突策略本站优先（同 mid/pid 跳过）；不同步折扣与 `group_ratios`；转发规则仅映射本站已有 eid/name，不创建；渠道不同步
- 管理端：本站密钥 / 拉取同步（测试连接、预览 Diff、确认同步）/ 同步日志；公开导出 `GET /api/v1/plugins/data_sync/export/bundle`

## 2026-07-30 — HA/选渠日志字段改中文描述

- `[HA]` / `[选渠]` / `[Chat]` 信息日志的 key 改为中文（状态码、上游YID、冷却等）；仅文案，逻辑与性能无实质影响

## 2026-07-30 — 上游失败状态码：日志 / HA / 客户端对齐

- `norm_status`：`record_zero_cost_fail` 落库与 `upstream_fail` 共用，避免日志写原码、对外被改成 502 的分叉；文案侧私有 `norm_err_msg`
- 火山 TTS 兼容路由：记账返回的推断状态码再构造 `upstream_fail`（原先日志推断、客户端常落 502）
- HA `first_fail` / reinstate / 熔断仍读同一 `UpstreamHttpError` 状态码；主路径 HTTP 非 2xx 行为不变

## 2026-07-30 — HA 熔断写入迁入 ha.rs

- `trigger_ha_meltdown` 从 `proxy.rs` 挪到 `ha.rs` 私有，与 `try_failover` / `scrub` 同模块；`proxy` 不再承载 HA 熔断
- 日志前缀统一 `[HA]`；判定统一 `is_ha_aid`；冷却表/白名单/周期 scrub 行为不变

## 2026-07-30 — 清理无调用启动种子化与误导配置

- 删除从未被 `main` 调用的 `Database::seed_admin`，以及仅为其服务的 `AppConfig.admin_username/password`
- 同步清理 `.env.example` / compose / `deploy.sh` / README 中的 `ADMIN_*`、`SEED_ADMIN_ON_BOOT`（管理员改由网页初始化页创建；运行时行为与改前一致）
- 去掉 hyperbc 多余 `#[allow(unused_imports)]`；全仓仍无 `#[cfg(test)]` / `#[test]` / `*.test.*`（保留产品页 ChannelTest）

## 2026-07-30 — relay 失败结算：规范锚点、暂缓再抽层

- `DEVELOPMENT_RULES.md` 补充零费用失败结算两入口 + `record_zero_cost_*` 选用表（行为零变更）
- 调用点样板已够用：再抽 builder / 合并判定会抬高回归风险；本仓不留存 `#[test]`，验证靠编译与手工路径

## 2026-07-29 — relay 零费用失败结算扩展复用

- `record_zero_cost_fail`：只记账，返回 `(status, client_msg)`，供 BadRequest / PaymentRequired 等非 `upstream_fail` 出口
- `ZeroCostUpstreamFail` 增加 `pre_deducted` / `pre_deduct_gift`、`upstream_req_content: Option`
- 已接入：image 同步轮询失败/超时/空图、video 素材转换失败记账、预扣费失败记账、audio 火山 TTS 错误记账
- **刻意未改**：成功计费、流结束结算、素材转换/预扣仍返回 BadRequest/PaymentRequired（避免变成 UpstreamHttpError 触发 HA）

## 2026-07-29 — relay 零费用上游失败结算收拢

- 新增 `proxy::record_zero_cost_upstream_fail` / `ZeroCostUpstreamFail`：两入口判定不变（HTTP 非 2xx vs body 业务失败），失败后统一记账 + `upstream_fail`
- 状态码：有 HTTP 失败码则优先用；否则从 body 推断。日志/客户端文案按原模块约定（含 chat 主路径短句、image/video post 的 format 体）
- 已接入 image / video / audio / generic / chat / native；流式成功结算与火山 TTS 专用路径未改

## 2026-07-29 — ha.rs 失败链收拢 + API 降可见性

- 删除薄包装 `on_spawn_fail` / `on_spawn_attempt_fail` / `on_attempt_fail`；上游失败逻辑并入 `HaAttempt::on_spawn_result_err`
- 仅内部使用的符号改为私有；对外保留 `HaAttempt` 与 `policy` / `yid_label` / `channel_is_ha_flag` / `resolve_log_config_id` / `is_melted_down` / `scrub_failed_channels`
- 行为不变：first_fail、400 系 skip-melt、业务侧 finish 优先

## 2026-07-29 — API 教程移除「火山素材库接口」菜单

- 默认文档树不再包含 `volcengine-assets-guide`；存量可在后台「恢复默认文档」清掉
- 不影响火山方舟 / MediaKit 教程及其他协议文档；素材库业务 API 本身未改动
- 精简 DocsApi：去掉增量种子/下线迁移与专用 seed 函数（Codex/Claude、级联指南、素材库），统一靠后台「恢复默认文档」对齐默认树；保留表结构/插件开关/slug/intl 迁移
- 复查：无 `#[cfg(test)]` / `#[test]` / `*.test.*` / `*.spec.*` 残留（保留产品页 ChannelTest）

## 2026-07-29 — 火山验证码变量名填写说明

- 管理端提示：控制台显示 `${1}` 则填 `1`，勿带 `${}`；按填写原样组 TemplateParam

## 2026-07-29 — 短信/验证码链路复查加固

- 火山国际号保留 E.164（不再误剥 `+`）；成功响应强制要求 MessageID
- 发送前校验消息组/SdkAppId 与签名；服务商切换文案改为「仅一套凭证」避免误解
- 验证码有效期统一为 `VERIFICATION_CODE_EXPIRY_MINUTES=5`（落库与邮件正文同源）
- 再扫：无单测残留；去掉仅内部转发的 `provider_kind`，HMAC 复用 `volcengine::hmac_sha256`

## 2026-07-29 — 短信通知支持火山引擎（原生 HTTP，无 SDK）

- `SmsSettings.provider`：`tencent`（默认，兼容旧配置）| `volcengine`
- 火山 SendSms：复用 `volcengine_sign`，消息组 ID 映射 `sdk_app_id`；验证码走 `TemplateParam` JSON（变量名 `code_param`，默认 `code`）；余额提醒无变量
- 腾讯云路径不变；管理端可切换服务商并分别配置凭证/模板/测试发送

## 2026-07-29 — 清理无用类型/导入与短信辅助拆分

- 全仓复查：仍无 `#[cfg(test)]` / `#[test]` / `*.test.*` / `*.spec.*`（保留产品页 ChannelTest）
- 删除未引用前端类型：`FinanceRechargeRecord`、`SMTPSettings`、`SmsSettings`、`GoogleOAuthSettings`、`WechatOAuthSettings`
- 去掉 EmailNotification / OAuthSettings 无用导入；`send_with_template` 改为私有
- `apiErrMsg` / `SKIP_ERR` 抽到 `utils/apiErr.ts`，短信提示组件只负责模板文案

## 2026-07-29 — 营销「提示通知」去掉重复短信模板文案

- 完整模板申请正文仅保留在「站点设置 → 消息通知 → 短信」
- 营销侧只保留开关、缺模板一行警告，以及指向短信配置的简短说明

## 2026-07-29 — 余额提醒短信改为无变量模板

- 腾讯云余额提醒模板不再传 `TemplateParamSet`（避免 `TemplateParamSetNotMatchApprovedTemplate`）
- 测试短信只传手机号；邮件测试仍可用余额/阈值预览变量
- 管理端文案与可复制申请正文同步为无变量固定模板

## 2026-07-29 — 清理无调用死代码（无单测残留）

- 全仓复查：无 `#[cfg(test)]` / `#[test]` / `*.test.*` / `*.spec.*`（保留产品页 ChannelTest）
- 删除未使用：`BillingIngress::enqueue`、`ApiToken::has_quota`、`BillingRule::apply_time_multiplier`、`Model::get_group_ratios`/`get_multiplier_for_group`、`tos::get_object_tags`

## 2026-07-29 — HA 终态与转发规则路径收紧

- `finish_err`：业务侧错误（余额/鉴权/BadRequest）优先于 first_fail，避免上游失败后被 402/400 盖住仍回上游文案
- image/video/audio/generic：转发规则不匹配改为 `on_access_err`+`break`（与 chat 一致，走 `ha.finish()`）
- 删除无调用的 `HaAttempt::reset_attempts`；熔断路径 yid 只格式化一次

## 2026-07-29 — 选渠/上游/ModelMeta 日志补回 info

- `[SelectChannel] start|db_candidates|after_filter`、`[Image→Upstream]`、`[ModelMeta]` 从 debug 升回 info（默认 `RUST_LOG=info` 可看）；preset / Image path=* 等仍 debug

## 2026-07-29 — chat_completions 去掉 EP 多模型空壳

- 删除恒为单元素的 `model_list` for、恒 `None` 的 `ep_tag` / `resolved_ep_models`；权限校验一次后直接进 HA while（与 responses 同构）

## 2026-07-29 — chat HA 失败路径与其它 relay 统一

- `chat_completions` 去掉平行的 `try_failover` / `trigger_ha_meltdown`；改为记账后 `HaAttempt::on_spawn_result_err`（与 image / responses 同构）
- 非首次失败由 `reinstate_first_log` 还原 first_fail；删除 `chat_upstream_fail` / `record_failed_billing`

## 2026-07-29 — HA 同档子渠按绑定序（不再随机）【本条已过期，见下方订正】

- 子渠选中：`priority` 降序 → 同档 `weight` 降序 → 再同则按 `sub_channels` 绑定序；去掉加权随机，错误终态稳定
- 物理渠之间的优先级/权重选渠逻辑不变；failover / first_fail 语义不变
- **订正（2026-11-04 核对代码）**：2026-08-10 `3dbe5cb` 已把同档改回**加权随机**（抽出 `pick_weighted_by`），当前生效的是按 `weight` 比例随机分流，不是确定性绑定序。确定性版本的动机「错误终态稳定」随之失去：全组失败时「第一次失败的上游」每次可能不同，日志对不齐。照本条原文实现会做错。

## 2026-07-29 — 本地默认 info；HA 关键日志升为 info

- `dev.sh` 等默认 `RUST_LOG=info`；需要明细时 `RUST_LOG=debug ./dev.sh`
- HA begin/switch/stop/finish/熔断/skip-melt 与选渠 `picked` 改为 `info`（无级别分支；异常 reinstate/软上限仍 `warn`）

## 2026-07-29 — HA/选渠日志补 yid

- `[SelectChannel] picked`、`[HA] switch/stop`、熔断/白名单日志增加 `yid=`，便于对照后台上游 YID（原先只有 config id / aid）

## 2026-07-29 — 本地开发默认 RUST_LOG=debug

- （已由同日「默认 info」条目取代）原将 `dev.sh` 等默认改为 `RUST_LOG=debug`；现改回默认 `info`，debug 需显式开启

## 2026-07-28 — 删除 TRUSTED_PROXIES（整项废弃）

- 去掉环境变量 / `AppConfig` / compose / `.env.example` / README 中全部 `TRUSTED_PROXIES` 说明，避免误配
- `extract_client_ip`：优先 `X-Forwarded-For`（首段）/ `X-Real-IP`，否则 socket IP；与 Docker 反代取真实 IP 行为一致

## 2026-07-28 — Mac 导出逻辑精简与加速

- `export-images.sh` / `push-images.sh` 内联 zigbuild + PREBUILT（不抽公共脚本文件）
- 预编译镜像阶段去掉 `apt install file`；`USE_PREBUILT=1` 时 frontend+backend 并行
- 仓库在 `/Volumes` 时 zigbuild 的 `CARGO_TARGET_DIR` 迁到本机 SSD 缓存；开关与用法不变

## 2026-07-28 — zigbuild 交叉编译启用 vendored OpenSSL

- 根因：`cargo zigbuild` 到 `*-linux-gnu` 时 `openssl-sys` 找不到 Linux OpenSSL sysroot 而失败，无法生成 `tokensbyte-server-bin`
- 新增可选 feature `cross_compile`（`openssl` vendored）；导出/推送脚本的 zigbuild 自动带上
- 日常与 Docker 内编译默认不启，依赖面不变

## 2026-07-28 — 计费入口去掉重复 round

- `check_access` 直接用 `ctx.balance`（`get_user_context` 已 round）；注释微收紧

## 2026-07-28 — 结算差额口径抽到 money::settlement_delta

- 同步 `record_and_bill_inner` 与异步 `execute_settlement_tx` 共用 `settlement_delta`；预扣金额读库后 `round_money`
- 行为不变：应付原样落账、赠送优先、余额可负

## 2026-07-28 — 修复计费 cost=0 与空钱包仍可调用

- 入口：`check_access` 对有/无限额令牌统一校验可用额（系统+赠送+信控，`round_money`）；预扣=0 且可用≤0 → 402
- 结算：同步/异步去掉授信封顶下调 `settled_cost`；应付金额原样写入 `logs.cost`，赠送优先后 `balance` 可扣成负
- 删除无用 `cap_additional_charge`；避免「有计费依据却 cost=0」导致平台亏费

## 2026-07-28 — 素材转换判定微精简

- `is_base64_media`：去掉与「含 `:` 即非纯 base64」重复的 http/asset 分支；判定结果不变
- 复用已够（共用解码/TOS/Create）；按规范不新增留存单测，不做更大抽层

## 2026-07-28 — 素材转换支持纯 base64

- 插件/上游素材转换：`content` 中除 `data:` URI 外，亦接受无前缀纯 base64（判定与 `forward::parse_image_data` 对齐）
- 共用 `is_base64_media` + `decode_base64_data`（魔数推扩展名，未知则按 Image/Video/Audio 默认）；http(s) 路径不变

## 2026-07-28 — 清理无用代码与 scratch 脚本

- 全仓确认无 `#[cfg(test)]` / `#[test]` / `*.test.*` / `*.spec.*`（保留产品页 ChannelTest）
- `PluginConfig`：去掉未用 `ApartmentOutlined`、死别名 `TOS_REGIONS`；`INDEPENDENT_STORAGE_PLUGINS` 提升为模块常量
- 删除本地 scratch `scratch_restore.sh`，并去掉 opensource 打包对其排除项

## 2026-07-28 — 素材转换/存储配置微提炼

- `asset_convert`：成功写回复用 `push_convert_ok`；行为与日志格式不变
- `PluginConfig`：独立存储插件名单收敛为 `INDEPENDENT_STORAGE_PLUGINS`（含 upstream_asset_relay）
- 按仓库精简规范：不新增留存单测

## 2026-07-28 — 上游素材插件存储改为可单独配置

- 「火山视频转素材ID」存储配置改为与素材中心同类的独立 TOS 表单（可保存/测试连接）
- 未单独配置时运行时仍回退「站点设置 → 存储设置」全局 TOS，不影响已有 base64 转换

## 2026-07-28 — 上游素材插件增加全局存储配置页

- 「火山视频转素材ID」插件配置增加「存储配置」页签（后续已改为可单独配置，见上条）
- 便于确认 base64→TOS 临时上传所用存储来源；运行时仍走原有 `get_tos_config` 回退，不影响转换逻辑

## 2026-07-28 — 上游素材转换支持 base64（与插件路径统一）

- `upstream_asset_convert`：base64 data URI 与 `asset_convert` 共用「解码 → TOS 临时 URL → CreateAsset(URL) → asset:// → 清理临时对象」
- 抽取 `convert_base64_with_create` / `create_asset_via_upstream`，去掉上游「不支持 base64」分支；需系统或插件 TOS

## 2026-07-28 — Mac 导出强制 zigbuild，杜绝 Docker cargo OOM

- 根因：zigbuild 失败/跳过后仍继续容器内 `cargo build --release`，易 SIGKILL / cannot allocate memory
- `export-images.sh` / `push-images.sh`：Mac 必须先产出匹配架构 Linux ELF；`docker compose build --build-arg USE_PREBUILT=…`；仅 `ALLOW_DOCKER_CARGO=1` 才允许容器内编译
- `backend/Dockerfile`：构建阶段显式 `ARG USE_PREBUILT` 选择预编译路径；README / Mac 指南同步

## 2026-07-29 — format_as_openai_error 去重

- 门禁只留 `is_upstream_error_response`；去掉重复的 `/error/code` 手写解析与二次读 `error.message`

## 2026-07-29 — format_openai 错误分支去重

- 去掉外层重复的 `is_upstream_error_response`，仅经 `format_as_openai_error` 一次门禁

## 2026-07-29 — 上游错误 OpenAI 规范化收口

- `format_as_openai_error`：厂商错误转换 + 脏 OpenAI（`success`/数字 type）一并规范化
- `normalize_upstream_error_for_client` 缩为 parse → formatter，去掉重复 ErrorCode/error.message 分支

## 2026-07-29 — HA 切换停环修复与错误 OpenAI 格式

- 上游 400/403/422：HA **仍切换**下一子渠，仅跳过熔断（原逻辑直接停环导致绑 3 个只打 2 个）
- 上游错误对外统一收成标准 OpenAI `error{message,type,code}`，去掉网关 `success` 等字段
- 精简 HA 调试日志：`[HA] begin/switch/stop/finish`；SelectChannel/Image/ModelMeta 降为 debug

## 2026-07-28 — 修复 logs.is_ha 解码类型

- `is_ha` 列为 `INTEGER`（INT4），模型字段由 `i16` 改为 `i32`，消除 sqlx 解码报错

## 2026-07-28 — 日志 HA 标志改为写时快照

- `logs.is_ha`：请求当时是否走高可用组；预记录/结算/渠道测试写入，读列表用快照不再 JOIN `channels.provider_type`
- 避免事后把普通渠道改成 HA 组后，历史非 HA 日志被误标；存量日志默认 `is_ha=0`（保守不误导）
- `logs` / `logs_archive` 各 `ADD COLUMN is_ha`；归档改为 jsonb 按列名写入，不再依赖 `SELECT l.*, NOW()` 列序
- `logs_archive_v1` 主键幂等改为 `EXCEPTION WHEN duplicate_object`（等价更短；已执行环境仍跳过该 ID）

## 2026-07-28 — Mac 导出镜像自动 cargo-zigbuild 预编译

- `export-images.sh` / `push-images.sh`：Darwin 选定目标 arch 后自动 `cargo zigbuild` → `tokensbyte-server-bin` → `USE_PREBUILT=1`（后续已改为失败即停，见上条）
- README 与 `docs/Mac开发环境打包与交叉编译指南.md` 同步

## 2026-07-28 — 移除未使用 TipTap 与直接 openssl 依赖

- 前端：删除未被引用的 `TipTapEditor` 及全部 `@tiptap/*` 依赖、相关 CSS；富文本仍用 `react-quill-new` / `md-editor-rt`
- 后端：去掉 `Cargo.toml` 中无业务引用的直接 `openssl`（vendored）；Linux 上仍可由 `reqwest`→`native-tls` 传递引入系统 OpenSSL，不影响 TLS
- 清空 knip 对 TipTap 的 ignore；功能路径不变

## 2026-07-28 — 合并 cgdev0726 门户与 DOCS 增强

- 合并 `origin/cgdev0726`：保留赛博门户 GitHub 入口、游客 PluginRoute、site_portal_pro DOCS/模板与风格切换等改动
- 保留本分支 Chat/Anthropic 文档拆分与用户端日志脱敏；合并时去掉对方带入的 `#[cfg(test)]`（含 site_icons / site_portal_pro）
- 文档页样式类：门户用 `cyber-hacker-docs`，API 教程用 `docs-api-system`

## 2026-07-28 — 用户端日志脱敏与响应精简

- 普通用户 `/logs`、`/logs/{id}/detail`、`/task_logs`、仪表盘最近活动：不返回上游出参；计费明细去掉渠道/模型映射；清空 PID/EID/YID；`plugin_tag` 仅保留 `client_ct`/`title`
- 抽取 `redact_request_log_for_user` / `user_allow_view_log_details` 等复用；详情级联脱敏仍读库原 `plugin_tag`，不再折叠已隐藏的上游出参
- 超管后台与库内完整字段不变；全仓无 `#[test]` / `*.test.*` 残留（保留产品页 ChannelTest）

## 2026-07-28 — 日志错误码下拉补 422 并多语言

- 常用错误码选项增加上游常见 `422`；文案迁入 zh/en/ja/ko/vi 的 `logs.status_code_*`
- 占位符 `logs.search_status_code` 同步五语；选项构建去掉多余 `useMemo`（14 项直接 map）

---

## 2026-07-28 — 钱包余额不足统一返回/落库 402

- 新增 `AppError::PaymentRequired` → HTTP 402
- `check_access` / `pre_deduct_or_intercept` 余额不足由 403 Forbidden 改为 402；HA 仍禁止 failover，且 402 不触发渠道熔断
- 抽出 `ha::is_client_side_http_status`（400/402/403/422）供 failover / 熔断共用，去掉重复字面量
- Playground 持久化重试将 402 视为业务永久错误（与原 403 余额不足行为对齐）
- 令牌额度耗尽、模型未授权等权限类错误仍为 403，行为不变

---

## 2026-07-28 — 今日改动复查修补

- 日志错误码 Select 清空时归一为 `undefined`，避免 antd 传入 `null`
- 主题推广有效期：前端改传 ISO（含时区）；后端无偏移墙钟按站点默认 timedisplay 解析，修正原「按 UTC 解读」偏差
- 主题推广点击统计 key / 上报 slug 统一小写，与入库及团队邀请展示对齐
- 删除合并带入的 `#[cfg(test)]`（auth / theme_promo / link_click）；全仓复查仍无 `#[test]` / `*.test.*` / `*.spec.*`（保留产品页 ChannelTest）

---

## 2026-07-28 — 日志错误码改为常用码下拉

- 筛选栏「错误码」由数字输入改为常用状态码 Select（含 0/4xx/5xx），可搜索、可清空
- 仍精确匹配 `status_code`；查询/导出/重置行为不变

---

## 2026-07-28 — 合并 cgdev0714time 主题推广与邀请追踪

- 并入主题推广落地页、营销链接点击统计、邀请 1 天锁定与 OAuth 归因、`dev.sh` Mac 编译优化
- 保留本分支日志错误码筛选、API 文档 Chat/Anthropic 拆分、方舟监控主题适配；团队选人仍走 `/users?keyword=`（不恢复已删的 `search-users`）

---

## 2026-07-28 — 方舟监控凭证/绑定页去掉白底卡片

- 主账号凭证、Endpoint 绑定去掉多余 Card 白底，与页面背景融为一体（白天主题不再套白块）

---

## 2026-07-28 — 方舟监控绑定/账号页主题适配

- Endpoint 绑定、主账号页去掉硬编码浅色字色与主按钮渐变，改用 `theme.useToken`
- 次要信息用 `colorTextTertiary`；等宽 ID/密钥用 `fontFamilyCode` 的 span，避免暗色主题对比失效

---

## 2026-07-28 — 清理调试日志与未用导入

- 去掉 WalletDetailsView 每次渲染的 debug `console.log`
- PluginsList 插件列表拉取失败改为静默兜底（功能不变）
- Logs 页移除未使用的 antd/lucide/`CardActions` 导入
- 全仓确认无 `#[test]` / `*.test.*` / `*.spec.*` 残留（保留产品页 ChannelTest）

---

## 2026-07-28 — 日志筛选参数构建去重

- `buildLogListParams` 统一列表/导出拼参；`pickFilter` 正确处理重置时显式 `undefined` 覆盖
- 精简 `parseStatusCodeFilter`；临时校验后无测试残留

---

## 2026-07-28 — 日志页支持错误码精确筛选

- `LogQuery` 新增 `status_code`，WHERE 精确匹配 `logs.status_code`；与原有 success/fail 互不干扰，导出共用同一条件
- 日志筛选栏增加「错误码」输入（仅数字），查询/回车/导出均生效；重置一并清空

---

## 2026-07-27 — Anthropic 聊天文档仅保留 base64 图片

- 确认网关 Anthropic 透传不转换图片；文档/PDF 无接入处理 → 不写
- 去掉图片 URL 示例，多模态仅保留 base64（含多图）；与线上可用方式对齐

---

## 2026-07-27 — 聊天文档去掉无效可选字段

- 删除 `image_url.detail`（网关未处理；协议转换只取 `url`）
- Anthropic 文档去掉未接入的 `document`/PDF 说明，参数表仅保留实际用到的字段

---

## 2026-07-27 — 常用示例拆分聊天 Chat / Anthropic

- 「常用调用示例」原合并篇拆为 `聊天 Chat`（OpenAI 兼容）与 `聊天 Anthropic`（原生 Messages）
- Chat：纯文本 / 图片 / 音频 / 视频 curl；Anthropic：纯文本 / URL·Base64 图片 / 多图 / PDF 简介
- 删除旧 `claude-chat` 种子；docs_api 与 site_portal_pro 同步

---

## 2026-07-27 — 图像 API 默认文档去掉 SDK tabs

- docs_api / site_portal_pro 默认文档中图像示例（gpt/volc/kling/google）去掉 Python/Node tabs，仅保留 curl
- `common-errors` 错误类型 tabs 保留；zh 图像编辑仍保留 JSON + Form-Data curl

---

## 2026-07-27 — 精简聊天示例并补齐 Anthropic 多模态

- 「常用调用示例 / 聊天对话」去掉冗长 SDK tabs 与响应样例，改为 curl 纯文本 + 多模态
- OpenAI 与 Anthropic 原生均补充单图 + 文本；Anthropic 注明 URL / base64 `source` 写法

---

## 2026-07-27 — API 教程聊天示例补充多模态单图

- 「常用调用示例 / 聊天对话」增加 OpenAI 兼容单图 + 文本（URL）curl 示例；参数表注明 `content` 可为多模态数组

---

## 2026-07-27 — 今日模型搜索改动复查修补

- `/models` 的 `page_size`：忽略 ≤0、上限 10000，避免异常 LIMIT
- Models 列表写入用 `Array.isArray(data)` 兜底，防止畸形响应导致渲染崩溃
- ModelSelector 类型补 `model_id_alias`，与共用关键词匹配字段对齐

---

## 2026-07-27 — 清理死状态与确认无测试残留

- 全仓确认无 `#[cfg(test)]` / `#[test]` / 前端 `*.test.*` 残留（保留产品页 ChannelTest）
- Models 删除从未读写的 `classStats` 及无用类型导入

---

## 2026-07-27 — 解耦 API 教程与站点门户增强版文档数据源

- `/docs`（docs_api）不再优先请求 `site-portal-pro`；空树也不再误占位导致「暂无文档」
- 路由显式绑定：`/docs` → docs-api，`/home-pro/docs` → site-portal-pro；两套库表与开关互不影响
- 共享 `RelayAPI` 仅按传入 `apiPrefix` 取数，去掉跨插件 try/catch 回退

---

## 2026-07-27 — 模型筛选参数/绑定再提炼

- 抽出 `buildClassificationParams`，Models / PluginConfig 共用
- `list_models` 筛选绑定抽 `model_filter_ids`，与 WHERE 顺序一致、去重 list/count 绑定
- 模型广场本地搜复用 `modelMatchesKeyword`（保留 original_id / provider / description / variants）
- 本链路无单元测试残留；不硬抽 stats 三段 SQL（JOIN 交叉条件易改坏角标）

---

## 2026-07-27 — 模型搜索链路复查修补

- `/models` 无 page_size 时用列表长度作 total，去掉多余 COUNT
- Models 分类请求加 QueryGuard，避免快切筛选时旧响应覆盖新数据
- 抽出 `modelMatchesKeyword` 统一管理端模型本地搜字段；PluginConfig 分类判断改 `!= null`；团队选人 keyword trim

---

## 2026-07-27 — 同类搜索冗余一并精简

- 删除废弃 `Channels.tsx.patch`
- 团队营销选人改为复用 `/users?keyword=`，移除重复的 `/team-marketing/search-users`
- 插件配置 / ModelSelector 本地模型搜对齐备注字段；stats/models 服务端 search 已在此前去掉

---

## 2026-07-27 — 去掉模型列表/分类统计的服务端 search

- 管理端列表已本地搜，删除 `/models` 与 `/classifications/stats` 的 `search` 参数及相关 SQL
- 插件配置页 stats 不再跟搜索关键词；列表仍本地筛，分类变化才刷角标

---

## 2026-07-27 — 模型管理搜索失效修复（备注 / 旧后端）

- 列表 `/models` 按分类拉取，关键词（含备注）本地筛；打字不打 API

---

## 2026-07-27 — 模型管理搜索归 /models，stats 不再跟 search

- 列表与 stats 职责拆分

---

## 2026-07-27 — 修复模型管理备注搜索不生效

- 列表关键词本地筛选（名称 / Model ID / MID / 别名 / 备注）

---

## 2026-07-27 — 清理单元测试与仅测用代码

- 移除 backend 全部 `#[cfg(test)]` 测试模块（forward / live_metrics / runtime_info / site_portal / site_portal_pro）
- 删除 site_portal_pro 中已标注「仅单测保留」的托管首页辅助函数；生产路径（自定义主页 / 风格首页）不变
- 保留产品功能页（如通道测试 ChannelTest）与浏览器测试 skill

---

## 2026-07-27 — 模型管理列表支持按备注搜索

- 管理端模型列表与分类统计的 `search` 增加 `remark ILIKE`；搜索条件抽成共用片段，列表/COUNT 共用 WHERE，避免多处漂移
- 搜索框占位补充「备注」

---

## 2026-07-27 — 级联进行中：用户端日志/任务列表掩码阶段一成功结果

- 普通用户端：级联未完成时详情响应强制为处理中形态，并硬保证不含 http(s) 产物地址
- 仅在 `plugin_tag` 含 cascade 时修补目标分辨率（避免无关 plugin_tag 被默认成 720p）
- 任务列表对未完成任务不返回预览 URL；管理员与完成后行为不变

## 2026-07-28 — 赛博顶栏操作区尺寸对齐

- 语言/矩阵圆钮统一 36×36，登录/注册同高同圆角；双语占位改为同行叠字，避免 Login 被撑高

---

## 2026-07-28 — 赛博语言按钮显示当前语种

- 顶栏语言钮改为显示当前语言（中文页「中」、英文页「EN」），不再显示「即将切换到」的语种

---

## 2026-07-28 — 赛博登录/注册按钮中英等宽

- 默认文案改为 `登录|Login`、`注册|Sign Up`（去掉过长的 Get API Key）；旧配置自动迁移
- 顶栏与 hero 按钮用中英双文案占位，切换语言时宽度不跳动

---

## 2026-07-28 — 赛博门户首页 Hero 文案改写

- 主标题改为「大模型 API 中转开源系统，一处接入全球模型」；副标题突出 OpenAI 兼容、密钥/限流/路由/计费与可自托管

---

## 2026-07-28 — 赛博顶栏喇叭改为中英语言切换

- 右上角原音效按钮改为 `中` / `EN` 切换，写入 `localStorage.lang`，与门户双语 `|` 文案一致
- 首页硬编码区块补齐中英 `data-i18n-dynamic`；矩阵雨保留，音效开关移除

---

## 2026-07-28 — 赛博子页顶栏与首页共用同一组件

- 抽出 `_chrome_header` / styles / scripts；首页与模型广场等子页共用同一套顶栏 HTML（含音效/矩阵按钮与 CRT 底纹），不再用 CSS 近似

---

## 2026-07-28 — 门户模型广场顶栏对齐首页

- 赛博黑客风格下，模型广场等子页顶栏与首页一致：Orbitron 品牌、脉冲方标、`//` 导航、霓虹登录/注册按钮

---

## 2026-07-28 — 门户模型图标统一解析与回退

- 模型列表/详情图标统一：模型 logo → 服务商 → 类型 → lobe 默认图；裸名自动拼 `/assets/icons/lobe/*.svg`
- 加载失败降级到默认图，再失败显示字母占位，避免裂图或不显示

---

## 2026-07-28 — 门户模型页 UI 优化与死链清理

- `/home-pro/models` 列表页重排：粘性筛选、结果计数、移动端筛选按钮，并适配 cyber_hacker / tech / dark_gradient
- Docker/Nginx 不再把门户模型页打到 SPA；底部默认链接改为真实门户路径，空/#/旧锚点死链不渲染

---

## 2026-07-28 — 门户管理配置对各风格页生效

- 共享 `base.html` 底部改为读取门户管理扁平 `footer_config`（品牌/链接/资讯/法律信息），经典/科技/星空/赛博子页均生效
- 赛博黑客首页链接加 `| safe`，避免 `/` 被 HTML 转义导致登录注册等配置链接失效
- 补充各风格首页与关于页渲染单测，校验导航/底部/SEO/脚本注入

---

## 2026-07-28 — 门户导航模型广场改走 /home-pro/models

- 默认「模型广场」改为门户页 `/home-pro/models`（非站点插件 `/home/models`）
- 新增动态路由渲染门户模型列表；已保存 `marketplace` 菜单若仍指向 `/home/models` 会自动迁移

---

## 2026-07-28 — 门户导航管理增加预览按钮

- 顶部导航菜单每项在删除按钮前增加预览，新窗口打开对应链接（锚点拼到首页）

---

## 2026-07-28 — 门户默认导航精简为文档与模型广场

- 默认顶部导航移除「平台优势 / 核心功能 / 模型矩阵 / 接入指南」；仅保留「文档」「模型广场」
- 已保存配置中的上述首页锚点菜单加载时自动剔除

---

## 2026-07-28 — 门户默认导航增加 DOCS

- 顶部导航默认项新增「文档|Docs」→ `/home-pro/docs`；已保存配置若缺少该项会自动补上

---

## 2026-07-28 — 赛博黑客门户首页接入门户管理配置

- 赛博黑客风格首页读取门户管理中的导航 / 底部 / SEO / 自定义脚本（统计与客服），保存后生效到 `/home-pro`
- 仅首页走赛博整页模板；关于/联系等子页仍用共享 chrome，避免丢失配置导航

---

## 2026-07-27 — 用户端 DOCS 切换分类默认打开首篇

- 站点门户用户端 docs：点击顶部分类后自动打开该分类下第一篇文章；无 URL 时默认分类亦打开首篇

---

## 2026-07-27 — 渠道指南路由示意图排版修复

- 「渠道管理与负载均衡」文档去掉易乱码的 ASCII 框图，改为表格 + 层级列表说明优先级/权重调度

---

## 2026-07-27 — 站点门户 DOCS 分类可排序与设默认

- 分类管理弹窗支持上移/下移调整顺序、「设为默认」、编辑名称与排序值、增删分类
- 前后台分类列表按 `sort_order` 排序（默认分类不再强制置顶）

---

## 2026-07-27 — DocsApi API教程页去绿色强调色

- 用户端 `/docs` 翠绿强调改为 zinc 中性色，与侧栏/顶栏整体风格一致（分类选中、侧栏高亮、行内代码、引用、标题竖条等）

---

## 2026-07-27 — API文档标题去空格

- DocsApi / 站点门户 DOCS：中文种子标题去掉全部空格；用户端侧栏展示同步去空格（含已有库数据）

---

## 2026-07-27 — DocsApi API教程页顶栏对齐控制台

- 用户端 `/docs` API教程页右上角改为与登录后控制台一致：模型广场、API教程、主题切换、语言、通知、用户头像

---

## 2026-07-28 — 站点门户 DOCS 初始化数据对齐当前分类

- 「使用指南」初始化写入模型/渠道根级文章；「商务合作」写入商务合作文
- 「API 参考」目录树去掉模型/渠道（改归使用指南）；快速开始仅保留鉴权与端点

---

## 2026-07-28 — 站点门户 DOCS 品牌对齐赛博黑客主页

- DOCS 左上角品牌改为 Orbitron + 霓虹发光字 + 脉冲方标，与赛博黑客门户主页一致

---

## 2026-07-28 — 站点门户增强 DOCS 多语言

- 管理后台 DOCS：补齐语言页签、各语言标题/正文编辑、AI 一键翻译
- 用户端 `/home-pro/docs`：顶部右上角增加语言切换；树与正文按所选语言展示

---

## 2026-07-28 — 站点门户增强 DOCS 管理端改 shadcn 灰阶

- 管理后台「DOCS文档」去掉用户端赛博绿主题，改用与 DocsApi 一致的黑白灰 shadcn UI

---

## 2026-07-27 — 站点门户 DOCS「API参考」标题去空格

- 已并入「API文档标题去空格」：全部类别侧栏/种子标题去空格

---

## 2026-07-27 — 站点门户 DOCS 代码块配色分层

- 代码井改为更深冷青绿面板 + 薄荷字色，与正文霓虹绿区分；保留单层边框与顶栏复制区

---

## 2026-07-27 — 站点门户 DOCS 代码块去外边框

- 用户端 DOCS 正文代码区去掉双重外框：保留代码区本身单层边框，去掉再套一层大外边框

---

## 2026-07-27 — 站点门户 DOCS 手机端布局优化

- 用户端 DOCS：窄屏侧栏改为浮层抽屉 + 遮罩；默认收起；点文档自动关闭
- 顶部导航：手机端安全区适配、分类横滑、略加深透明毛玻璃以保证可读性
- 正文：缩小边距与标题字号；表格/代码块横向滚动；修复 TOC 抽屉无法打开
- 断点切换：PC↔手机同步收展侧栏（无「展开再关闭」闪动）；回到 PC 默认展开菜单

---

## 2026-07-27 — 站点门户 DOCS 顶部导航透明毛玻璃

- 用户端门户预览 DOCS 页顶部导航恢复透明毛玻璃：排除实色 `bg-[#…]` 覆盖规则，半透明底 + `backdrop-filter` 模糊

---

## 2026-07-27 — 安全加固（鉴权 / CORS / XSS 面）

- Site Icons：管理接口强制 DB 校验 admin；图标名白名单；写入路径禁止 `..`；SVG 拒绝 script/事件属性
- 登录：`redirect` 仅允许站内相对路径；OAuth 改为一次性 `code` 兑换 JWT；代入登录用 localStorage handoff，避免 JWT 进 URL
- CORS：非 `APP_ENV=development|dev` 且未设置 `CORS_ORIGINS` 时拒绝跨域；开发环境仍可 permissive
- 主题推广 iframe：去掉 `allow-same-origin`，防止营销 HTML 读取父页 `localStorage`
- 登录/管理登录按 IP 每分钟 10 次限流

---

## 2026-07-27 — 高级营销：推广链接点击统计

- 专属邀请 / 团队邀请 / 主题推广链接前展示累计点击次数
- 统计规则：同一 IP 在站点时区自然日内对同一链接只计 1 次；已登录推广员访问自己的链接不计次
- 主题页失效回退注册页时只记主题链接点击，避免重复计入邀请链接

---

## 2026-07-27 — 推广邀请 1 天锁定不被覆盖

- 首次通过邀请链接写入 `aff`/`team` 后，**1 天内**不接受其他推广链接覆盖；**3 天内**注册仍可归因
- 锁定期内即使打开新的邀请 URL，注册/OAuth 仍使用已锁定的业务员

---

## 2026-07-27 — 邀请参数统一捕获与 OAuth 归因

- 统一 `aff`/`team` 捕获与读取（localStorage + cookie，3 天 TTL），覆盖 `/register`、`/promo/{slug}`、登录页「去注册」、表单注册
- 微信/谷歌 OAuth 自动注册：邀请参数写入 HMAC `state`（query/cookie 发起时嵌入），回调优先读 state、cookie 兜底，写入 `referred_by`、邀请奖励并自动入团
- 主题推广 HTML 内 `/register` 链接自动补上当前邀请参数

---

## 2026-07-27 — 优化 Mac 本地开发编译（dev.sh）

- 仓库在 `/Volumes` 外置盘时，自动将 `CARGO_TARGET_DIR` 迁到本机 `~/Library/Caches/tokensbyte-dev/target/`
- macOS 默认自动 `brew install sccache`（不自动装巨型 `llvm`）；若本机已有 `ld64.lld` 仍启用链接加速
- `[profile.dev]` 恢复 `incremental = true`，加速 cargo-watch 热重载

---

## 2026-07-27 — 高级营销：主题推广落地页

- 站点插件「团队营销管理」新增「主题推广」Tab：可粘贴 HTML 单页、设置上线/下线与长期或时段有效期
- 公开地址 `/promo/{slug}?aff=`；活动无效时自动回退普通邀请注册页
- 用户端「高级营销」在专属邀请链接下展示可复制的主题推广链接
- `theme_promotions.status` / `is_permanent` 升为 BIGINT，与项目 i64 约定对齐

---

## 2026-07-27 — 保护模型分类自定义排序不被升级覆盖

- 迁移回填「视频增强」logo/remark 时不再强制改写 `sort_order`；官方服务商 / API 服务商 / 类型的排序由管理端配置，升级须保留
- 迁移规范补充：种子与回填禁止顺带 SET 上述三表的 `sort_order`

---

## 2026-07-27 — 火山视频转素材ID：添加规则按关联名命名

- 「添加规则」生成的转发规则名固定为 `{关联名}·转素材#{绑定ID}`（如 `移动·转素材#3`），便于辨认归属且无需冲突探测；转换逻辑不变

---

## 2026-07-27 — 高可用最大备用切换次数上限调整为 100

- 插件「最大备用切换次数」UI/API 上限由 10 调整为 100；保存后仍即时加载到内存，逻辑不变

---

## 2026-07-27 — 修复用户端「视频监控」菜单默认不显示

- 用户菜单项 `/ark-video-monitor` 默认改为启用；插件开启后自动出现在侧栏（仍受插件开关与菜单配置约束）

---

## 2026-07-27 — 修复方舟监控 cron 因 is_enabled 类型崩溃

- `CronArkMonitor` 读取 `plugins.is_enabled` 由 `i32` 改为 `i64`（列已是 BIGINT），消除 INT4/INT8 解码失败导致同步中断

---

## 2026-07-27 — 级联转发规则支持 480p 目标

- 级联目标分辨率新增 `480p`：阶段一 480p 生成，阶段二 480p 画质增强（同分辨率增强，非超分）
- 转发规则可配 `480p` 倍率 / 增强 / 场景；底座锁定 `480p`（与 720p 档一致，UI 禁用切换）

---

## 2026-07-27 — 火山方舟视频监控改为钱包熔断

- 删除绑定表独立 `limit_quota`；同步时按 `used_quota - wallet_charged_quota` 增量扣用户钱包（赠送金优先，现金可扣成负数），写 `ark_video_consume` / `ark_video_refund` 流水
- 可用余额 `balance + gift_balance + credit_limit <= 0` 时停用该用户全部绑定并 `StopEndpoint`；余额恢复后 cron 仅自动 `StartEndpoint` 钱包熔断项
- 绑定表 `fuse_reason`：`wallet`=余额熔断可自动恢复，`manual`=管理员停用（cron 不拉起），启用时清空
- 上线后首次同步对历史 `used_quota` 追扣；管理端启用绑定需钱包可用余额 > 0；看板展示钱包可用与已用
- 同步按接入点批量 `SUM`；汇总查询失败则跳过该账号入账（避免失败被当成 0 误退款）；待恢复用户钱包批量读取

---

## 2026-07-25 — 本地 dev 默认开启 debug 日志

- `dev.sh` / `dev.ps1`（及 `dev-os.sh` / `dev-all.sh` / `local_restart.sh`）默认 `RUST_LOG=info`；`RUST_LOG=debug` 可覆盖。部署/`docker-compose` 默认仍为 `info`（见 2026-07-29 条目）

---

## 2026-07-25 — 支付回调敏感日志降级

- 通联回调原始 body、HyperBC 验签明细（公钥前缀/签名/待签串）由 `info` 改为 `debug`，与微信/支付宝一致；验签与入账逻辑不变

---

## 2026-07-25 — DeepSeek 缓存命中 usage 兜底

- `usage_extractor`：根级无 `cached_tokens` 时，将 `prompt_cache_hit_tokens` 映射为 `cached`（与 Claude `cache_read_input_tokens` 同级兜底）
- 不新增字段；`prompt_cache_miss_tokens` 仍由 `prompt - hit` 体现。需配置读缓存费率后折扣才生效，未配置行为不变

---

## 2026-07-25 — 阶梯计费文案精简

- 管理端说明、占位符、计费明细、模型广场改为更短的运营向文案（入/出/读/写）；计费逻辑不变
- 阶梯计费 / 豆包聊天阶梯说明进一步精简，并保留费用公式

---

## 2026-07-25 — GPT官方计费展示更名为 GPT图片计费

- 仅改 UI/日志文案与市场文案；`billing_rule=gpt_billing` 不变，已有规则与计费逻辑不受影响

---

## 2026-07-25 — 阶梯计费兼容 GPT 缓存写入

- `UsageTokens` 增加 `cache_write`；Chat (`prompt_tokens_details`) / Responses (`input_tokens_details`) 双路径提取 `cache_write_tokens`，不混入 Claude `cache_creation`
- `tiered` 阶梯每档可选 `cache_write_rate`：未填时行为与旧版一致（写入量并入未缓存输入）；填写后按写入价独立结算
- 管理端阶梯计费 UI / RateDisplay 展示「写缓存」选填列；保存时规范化 `cache_write_rate`
- 结算拆分逻辑去重为 `split_prompt_subsets`；提炼 `enrich_features_from_usage` 并在 Chat/Responses/原生/日志快照路径统一挂载 `cache_creation` 与 `web_search`
- 验证用临时单测已删除，不留测试残留

---

## 2026-07-24 — 图片上游 Content-Type 透传

- `image.rs`：去掉「gpt + 含图片」强制改 multipart；客户端用 `application/json` / `multipart/form-data` 则原样请求上游
- 提交类型写入 `logs.plugin_tag.client_ct`（预记录即落库；即梦轮询覆盖时保留），后台使用日志列表/详情可展示
- 提炼 `content_type_is_multipart` / `client_content_type` / `plugin_tag_*` 纯函数复用；前端日志页复用 `parsePluginTagMeta`（出参区不再重复打相同 Tag）
- tracing 仅记 `client_ct`（透传无第二套值）
- 删除仅服务旧强制逻辑的 `has_image_inputs`；验证用临时单测已移除，不留测试残留

---

## 2026-07-24 — 精简确认未用死代码（不影响功能）

- 删除未挂载的 `monitor/health.rs`、未引用的 `tos::download_file`、`VolcClient` 未调用的 `sign`/`hmac_sha256` 包装
- 删除未注册的 `examples/hash_pass.rs`；`auth` 改用已有 `hex` crate，去掉本地重复 `mod hex`

---

## 2026-07-24 — lettre 再收 feature（不影响发信）

- `tokio1-rustls-tls` → `rustls-tls`：业务只用同步 `SmtpTransport`，不再拉 lettre 的 tokio async SMTP 栈
- 仍保留 `builder` / `smtp-transport` / `hostname`（组信、SMTP、Message-ID）

---

## 2026-07-24 — Windows 打包脚本中文/路径兼容

- `export-images.ps1` / `push-images.ps1`：UTF-8 BOM + `chcp 65001`；固定切到脚本目录；中文/`PROJECT_NAME` 自动映射为 Docker 可用的 ASCII 名；非 ASCII 路径告警；`docker info` 检测；退出改 `ReadLine`（避免 `pause` 异常）
- 生成的 `import-images.ps1` 同样带 BOM；指南注明 Windows 打包机路径建议英文

---

## 2026-07-24 — 编译优化阶段 A–C（不影响功能）

- **A**：README / `export-images.sh`·`export-images.ps1` / `push-images.sh`·`push-images.ps1` / `dev.ps1` 写清 BuildKit cache、预编译 ELF、JOBS、勿删 target
- **B**：`axum` 去掉未用的 `ws`/`macros`；`futures` 收窄为 `std`+`async-await`；支付 `rsa` features 与 `reqwest` gzip/stream/multipart 保留（Windows/Mac/Linux 共用 `Cargo.toml`）
- **C**：直接依赖再审计无新增可删项；TOS 仍传递 `reqwest 0.11`（上游约束，不强行统一）；不为 TOS/邮件加额外 feature 门控以免误伤默认能力

---

## 2026-07-24 — 收窄 Cargo features / 删除确认未用直接依赖

- `tokio`：去掉 `full`，保留 default + `fs`/`signal`（覆盖现有 spawn/time/sync/net/fs/signal 用法）
- 去掉直接依赖 `tower`（曾 `features=["full"]`）、`axum-extra`、`eventsource-stream`、`pin-project-lite`、`bytes`（源码无引用；tower/bytes 仍由 axum 等传递）
- `tower-http`：仅保留实际使用的 `cors`/`fs`；`tracing-subscriber` 去掉未用的 `json`
- 功能路径不变；本机 `./dev.sh` / 镜像构建均可直接验证

---

## 2026-07-23 — 计费路径小修补（行为不变）

- 预扣退钱包：赠送退回钳在 `cost` 内，避免脏数据把 balance 打成负向
- 异步结算令牌强制落账后 `invalidate` 内存 slot，防止限额满时内存与 DB 漂移

---

## 2026-07-23 — 修复超龄冻结退款 `integer out of range`

根因：`logs.latency_ms` 为 INT4，超龄冻结 `EXTRACT(EPOCH)*1000` 直接 CAST 触发 PG 22003，退款事务回滚、僵死任务被反复捞起。结算/退款共用钳位表达式，功能与退款金额不变。

---

## 2026-07-23 — 精简无用代码（不影响功能）

- 确认并保持删除：`forward` / `upstream_asset_relay` / `upstream_asset_client` 内单元测试模块
- 去掉 `QuotaSlot`/`QuotaLimits` 从未读取的字段；去掉 `asset_convert` 仅转发的薄包装函数
- 保留产品能力：通道测试、存储/邮件等「测试连接」接口

---

## 2026-07-23 — Relay：计费闭环 3–8 点加固

- TaskPoller：2 日窗口、`id ASC` 分批；超窗冻结兜底退款
- BillingPipeline：刷库失败短重试后放回缓冲
- 令牌：限额已满仍强制落账（同步 + 异步结算差额）；授信封顶后 `logs.cost`/额度/钱包对齐
- 追加扣款：`cap_additional_charge`；HA 复用 pending 时预扣 reopen 为处理中
- 日统计：仅 `is_completed=1`；audio/generic 预扣改到上游成功后

---

## 2026-07-23 — Relay：预扣与 pending 日志同事务落账

`pre_deduct` 在扣钱包的同一事务写入 `logs.cost` / `pre_deduct_gift`，避免崩溃后「钱已扣、日志 cost=0」无法退款。孤儿清理/启动恢复抽公共 `close_pending_and_refund`；结算落库失败时对 pending 预扣立即 CAS 退回。去掉无用 `PreDeductSplit`，赠送优先拆分抽到 `money::split_gift_first`，钱包退款 SQL 复用。正常成功/失败/冻结结算金额语义不变。

---

## 2026-07-23 — Relay：超管/管理员与普通用户同等计费

取消管理员余额校验豁免与预扣跳过：`check_access` / `pre_deduct_or_intercept` 不再因 `role=admin` 放行；钱包预扣、终态扣费、令牌/渠道额度与普通用户同一路径。并移除 `pre_deduct_or_intercept` 的冗余 `role` 参数，以及 `UserContext` 中已无用的 `role` 字段与查询列。

---

## 2026-07-23 — 火山视频转素材ID：优化插件展示名

插件展示名由「上游素材中转」改为「火山视频转素材ID」，描述对齐火山视频自动转素材 ID 场景；内部 `name=upstream_asset_relay` 与路由不变。直接改本地种子 SQL（无增量迁移）。

---

## 2026-07-23 — 上游素材中转：去掉冗余代码

插件内合并绑定筛选公共逻辑；缓存列表不再取未展示的 `file_name`；生成规则响应去掉前端未用的 `config_json`；uid_map 查询抽公共函数。行为不变。

---

## 2026-07-23 — 修复火山 MediaKit「使用日志」慢查询与空列表

根因：`enhance-logs` 按裸 `plugin_tag IN ('vve-sd',…)` 过滤，但落库实为 JSON 字符串 `"vve-sd"`（或空 tag + `model`/`model_id`），COUNT 全表慢扫、列表常返回空。

处理：改为 `model = ANY(mid+model_id)`（走 `idx_logs_model_created`）或 `action_type = '视频增强'`；列表用 `deferred_join_page_sql` + mid/model_id 双键 LATERAL 取模型名；新写入 `plugin_tag` 改为 `{"mid":…}` 便于与 cascade 合并。接口字段与详情弹窗行为不变。

---

## 2026-07-23 — 上游素材中转：转换日志改为只读

对齐素材资产插件 API 日志：去掉单条/批量删除与对应后端 `POST /convert-logs/delete`；筛选、分页、刷新与展开详情不变。缓存管理仍在「素材缓存」Tab。

---

## 2026-07-23 — 上游素材中转：新增「素材缓存」Tab

对齐素材资产管理：可分页查看/筛选/单删/批删/清空本插件 `plugin_assets`（source=`upstream_relay_convert`）；与「转换日志」分离。删除仅清本地缓存，不影响 API 日志与转换能力。

---

## 2026-07-23 — 上游素材中转：区分转换日志与素材缓存

转换日志只读 `plugin_api_logs`；`plugin_assets`（source=`upstream_relay_convert`）是复用缓存，命中缓存时也可能不再写新日志。缓存查看/删除见「素材缓存」Tab。

---

## 2026-07-23 — 上游素材中转：减少可复用场景的重复查库

生成规则：已 JOIN 出的 `rule_name` 判断规则是否仍在，写回后复用内存中的 binding，不再二次 `fetch_binding`。运行时转换：插件启用与绑定/渠道一次 JOIN。转换日志 `total=0` 时跳过列表与 uid_map 查询。

---

## 2026-07-23 — 上游素材中转：移除契约单测残留

删除 `upstream_asset_relay` / `upstream_asset_client` 中的 `#[cfg(test)]` 契约测试；转换日志 scope 改回静态常量，避免每次请求 `format!`；运行时绑定/转换/日志能力不变。

---

## 2026-07-23 — 上游素材中转：去掉多余 api.ts，对齐 relay 常量

删除仅含一行前缀的 `api.ts`；`LOG_SOURCE`/`PLUGIN_NAME`/`binding_ns` 收拢到 `upstream_asset_client`，转换日志与写入侧共用；`asset_convert` 抽出 `shorten_url_for_log`，上游调用改用模块别名。

---

## 2026-07-23 — 上游素材中转：低风险精简

生成规则写回绑定抽公共函数；转换日志绑定名用一次映射。

---

## 2026-07-23 — 上游素材中转：转换日志筛选与分页加固

按 `binding_id` 筛选共用日志范围常量；列表页码按 total 钳制；删除后自动回退有效页；去掉无用 `source` 字段返回；生成规则命中已有绑定时不再重复查询。

---

## 2026-07-23 — 上游素材中转：转换日志按上游关联筛选

转换日志支持按绑定（`binding_id` → `plugin_name=uar:{id}`）筛选；列表展示关联名称；后端查询仍限定本插件日志范围。

---

## 2026-07-23 — 上游素材中转：转换日志对齐展示并支持删除

转换日志时间用 `formatApiDateTime`、用户展示 UID+用户名（`uid_map`）；去掉「来源」列；支持单条/批量删除（仅限本插件 `upstream_relay_convert` / `uar:%` 日志）。

---

## 2026-07-23 — 素材转换 Range 元数据超时改为 10s

指纹 Range 请求超时调整为 10s（到点才失败，提前返回不等满）；失败类型日志仍区分超时等。

---

## 2026-07-23 — 素材转换 Range 元数据失败日志区分超时

`fetch_meta_fingerprint` 请求失败时日志标明超时 / 连接失败 / 请求发送失败 / 其它，便于确认是否撞 5s 上限；超时时间暂不调整。

---

## 2026-07-23 — 素材转换指纹改用 GET Range 取元数据

`fetch_meta_fingerprint` 由 HEAD 改为 `GET Range: bytes=0-0`：整文件长度取 `Content-Range` 总长（源站忽略 Range 回 200 时回退 `Content-Length`），ETag/Last-Modified 不变；指纹公式仍为 `SHA-256(URL路径|长度|ETag|Last-Modified)`，与历史缓存兼容。L1/L2 复用规则不变。

---

## 2026-07-23 — 素材转换缓存：恢复「指纹未命中不查 URL」

统一 `lookup_cached_converted_asset`（原 `asset_convert` / 上游中转共用）：L1 指纹命中才复用；指纹已算出但对不上则跳过 URL 并重新注册（防同 URL 内容变更误复用）；仅指纹算不出时走 L2 URL。写入缓存失败打 warn。

---

## 2026-07-23 — 素材转换 CreateAsset 补齐 Name

转发素材转换与上游素材中转 CreateAsset 均补传 `Name`（由 URL 文件名推导，缺省则短哈希名），避免代理/上游报「素材名称不能为空」；落库 `file_name` 同步复用同一推导。

---

## 2026-07-23 — 上游素材中转：修复绑定保存 500（SQL 占位符）

根因：`fetch_binding` 追加 `WHERE id=?` 未走 `format_query`。统一列表/单条查询占位符转换；绑定字段沿用 `channel_config_id`（本地库已对齐）。Relay 仍 JOIN `channel_configs` 取 base_url/api_key。

---

## 2026-07-23 — 上游素材中转：关联列表对齐渠道预设展示与搜索

绑定列表/下拉改用「上游渠道配置预设」信息（名称、YID、服务商、base_url），支持按名称/YID/服务商/地址搜索快速添加；接口补充返回 `config_yid` / `config_provider_type`。仍绑定 `channel_configs.id`，不改运行时转换与旧 `asset_convert`。

---

## 2026-07-23 — 上游素材中转：清理验证单测残留

删除 `upstream_asset_relay` 落地时临时加入的单元测试（客户端 URL 拼装、规则 JSON、转发字段解析），保留管理端绑定/生成规则/运行时转换与日志能力；与现网 `asset_convert` 隔离不变。

---

## 2026-07-23 — 本地后端增量编译/链接加速（兼容 Windows）

- `[profile.dev]`：`debug = line-tables-only` + incremental。
- Windows：`dev.ps1` 默认设 `rust-lld`；`TOKENSBYTE_FAST_LINK=0` 回退 `link.exe`（不新增 `.cargo/config.toml`）。
- Linux：`dev.sh` 有 mold 则 `mold -run`，否则可选 `clang+lld`；macOS 不启用（`mold -run` 不支持）。
- 不改业务逻辑；无额外单测残留。

---

## 2026-07-23 — 级联转发规则：每分辨率可配增强版本与底座

- `config_json` 新增 `res_enhance`（fast|standard|pro|ai，默认 standard）、`res_scene`（标准版场景 common|ugc|short_series|aigc|old_film，默认 common）、`res_base`（阶段一座底，默认一级：720p→480p、1080p→720p、2k/4k→1080p）。
- 管理端级联配置按分辨率设置倍率/增强/场景（仅标准版显示）/底座；阶段二标准增强透传并校验 `scene`。
- 旧规则无新字段时增强默认标准、场景 common、底座一级，无需迁移；不保留级联相关验证单测残留。
- 级联 `version` / 目标 `resolution` 只写入 `plugin_tag.cascade`，不改用户入参；预扣费时用已有 `cascade_json_str` 写入 `billing_features`，终态直接复用快照。

---

## 2026-07-22 — 任务列表预览：列表返回产物地址，不受详情权限限制

根因：列表不带响应体，预览走详情接口；关闭「查看日志详情」时取不到媒体链。  
处理：`/task_logs` **一次查询**仅对图片/视频/视频增强读 `response_content`，内存提取 `preview_urls`（级联取 stage2，复用 `find_urls`，只回传 http(s)；日志内 base64 本已脱敏占位）后丢弃大字段；前端优先用 `preview_urls`，无详情权限时不再打详情兜底。详情权限仍约束完整展开，预览不受影响。

---

## 2026-07-22 — 新增系统增强插件「上游素材中转」

新增系统增强插件 `upstream_asset_relay`：可多条关联上游渠道配置与素材基础路径（路径可空），一键生成视频转发规则；模型选用后对请求内图片/音视频 URL 经渠道 Bearer 调用 CreateAsset/GetAsset 转为 `asset://`，带缓存与 `plugin_api_logs` 追溯。与现网 `asset_convert`（素材插件凭证）正交隔离，默认关闭，不影响原有行为。

---

## 2026-07-22 — 启动/定时清理：status_code=0 慢查询改走部分索引

`recover_interrupted_logs` / `cleanup_orphan_pending_logs` 原 SQL 对 400 万行 `logs` 全表扫（`status_code=0` + `NOT ILIKE '%冻结%'`，约 2s）。  
预记录默认 `is_completed=0`，改为先命中 `idx_logs_is_completed_pending` 再过滤；语义不变（异步冻结多为 `status_code=200`）。实测约亚毫秒级。

---

## 2026-07-22 — 修复 DbGate 卡 Loading structure（锁 + 损坏索引）

根因：多实例 StartupBackfill 长事务占 `logs` 锁，迁移非并发 `DROP INDEX` 与 DbGate 结构查询一起等待；另有损坏索引 `idx_logs_action_created_stats_new`（pg_attribute 缺口）干扰元数据。  
处理：运维侧已清损坏目录项并补齐 `idx_logs_created_at_agg`；`logs_indexes_reconcile_v1` 的 prune 增加 `lock_timeout=3s`，拿不到锁则跳过，不再堵库。

---

## 2026-07-22 — 去掉 migrations 验证单测

删除 `idempotent_index_ddl_tests`（`#[cfg(test)]` 不影响线上，但仓库不留验证残留）；幂等判断内联为单一私有函数，迁移终态与行为不变。

---

## 2026-07-22 — logs 索引迁移收口为 logs_indexes_reconcile_v1

- 旧 ID `logs_slow_query_indexes_v1` / `logs_created_at_agg_prune_v1` 改为 no-op（保留 history 兼容）。
- 新迁移 `logs_indexes_reconcile_v1` 为唯一终态：清 INVALID → 建 `idx_logs_created_at_agg` / `idx_logs_vision_created_at_new` → 尽力删冗余/损坏旧索引 → `ANALYZE`。
- `once_migration!` 仍对 CREATE 名冲突（23505）与 DROP 目录缺口（XX000）幂等跳过。重启后端执行一次即可。

---

## 2026-07-21 — 今日改动复查收口

- 腾讯视频：用户显式 `LastFrameUrl` 原样透传（可与 `FileInfos` 并存）
- 去掉 `PollTask` 内嵌套 `if is_tencent`
- 级联阶段二不再默认写 `bitrate_level: high`（恢复历史请求体，避免静默改变上游画质/计费）

---

## 2026-07-21 — 腾讯云 FileInfos 用户原样透传

用户已传 `FileInfos` 时图/视频 body 直接 `clone`，不做规范化改写；仅从 `images` 等兼容字段构建时仍走 `tc_file`。已删 `tc_norm_files`。

---

## 2026-07-21 — logs 日聚合索引补齐与冗余索引精简

- 新迁移 `logs_created_at_agg_prune_v1`（**未并入**已上远程的 `logs_slow_query_indexes_v1`，避免已执行环境跳过删冗余）
- 补齐 `idx_logs_created_at_agg`；删 `idx_logs_action_created_stats_new` 及旧名 `idx_logs_created_at*`（若有）
- 保留：pkey / log_id / task_id / user_id / action_created / vision / is_completed / status0；重启后端执行

---
## 2026-07-21 — 消除 LocalDayBounds.local_day dead_code 警告

`local_day` 仅赋值从未读取（调用方用 `start/end_utc` 或 RFC3339）；从结构体移除，边界计算不变。

---

## 2026-07-21 — 腾讯云 FileInfos 支持 Base64 输入

兼容字段（`images` 等）构建 `FileInfos`：base64 → `Type=Base64`+纯串，否则 `Url`（`tc_file`）。用户自带 `FileInfos` 见上方「原样透传」。

---

## 2026-07-21 — 清理无用单测与临时脚本

删除 `live_metrics` / `dashboard` 内 `#[cfg(test)]` 模块，以及根目录孤儿脚本 `test_plugins.js` / `test_plugins.sh`。不改业务路径；保留管理端「通道测试」等产品功能。

---

## 2026-07-21 — 级联阶段二防重复：DashMap 互斥

同进程 `cascade_s2_inflight` + RAII Guard（Drop 必 remove）。输家零查询 `InProgress`（对外仍走标准「进行中」）；无额外读库。裁剪固定角点 `(2,6)-(862,490)` / `(6,2)-(490,862)`。单实例部署，不做跨进程 CAS。

---

## 2026-07-21 — 手动轮询补齐腾讯云原始响应日志

根因：仅后台 `[TaskPoller]` 打印腾讯原始 body；用户手动 `[Task Poll]` 只打 `resp_len`。抽出 `log_tencent_poll_raw`，手动/自动/`PollTask` 共用；`is_tencent` 统一 `starts_with("tencent_vod")`。仅日志，结算与返回不变。

---

## 2026-07-21 — 腾讯视频终态分辨率覆盖计费特征

结算仅读终态 `Output.FileInfos[0].MetaData` 的 `Duration` / `Width` / `Height`（短边→480p/720p/1080p/2k/4k），覆盖请求侧任意分辨率；不读空/不可靠的 `Resolution`。在 `merge` 之后写入，避免被冲掉。

---

## 2026-07-21 — 级联阶段一 480p 非标居中裁剪

级联模式下阶段一为 480p 且 ratio 为 16:9/9:16 时，超分前走 MediaKit `crop-video` 居中裁成标准 480p（864×496→860×484 / 496×864→484×860）。逻辑在 `cascade.rs`，`task.rs` 阶段二提交复用；不命中不裁，失败回退原底座。

---

## 2026-07-21 — 启动慢 SQL：日统计回填 + 日志深翻页

### 现象
- 启动 `StartupBackfill`：`INSERT…SELECT` 聚合 `logs → usage_daily_stats` 超 1s（周批次可扫数十万行）。
- 日志列表：视觉类筛选 + 大 `OFFSET` 时，对 OFFSET 前全部行做 5 表 JOIN 与 `regexp_match(billing_detail)`。

### 改动（行为不变）
- `usage_stats`：按日 upsert；SQL 循环外编译复用；`FILTER` + `GROUP BY 1..6`；失败中止；仅日间 sleep。
- `list_logs` / `list_task_logs`：共用 `deferred_join_page_sql`；视觉 `= ANY(...)`；排序仅 `created_at DESC`。
- 迁移 `logs_slow_query_indexes_v1`：`idx_logs_created_at_agg`、`idx_logs_vision_created_at_new`。
- 验证用临时单测已删除，不留仓库残留。

---

## 2026-07-21 — 打包脚本精简原则

- **不抽** `docker-build-env` 公共文件（曾引入引用/漏改风险）；sh 与 ps1 各自独立、规则对齐即可。
- **不加** Docker 打包常驻单测（需本机 Docker/长编译，易碎；验证用 `bash -n` + 实跑导出）。
- `push-images.sh` 与 `export-images.sh`：Apple Silicon 默认 arm64，避免推送路径默认踩 QEMU。

---

## 2026-07-21 — 本地打包提速（不影响线上）

### 原则
只改构建过程；运行时镜像仍是 release Linux ELF + nginx，部署/更新方式不变。

### 改动
- 构建顺序：`frontend` → `backend`（先轻后重，给 Rust 腾内存）。
- 编译阶段启用 `lld`（仅链接加速）。
- `EXPORT_FAST=1` → Mac 上 JOBS=2；`SKIP_BUILD=1` → 只导出已有镜像。
- 二次构建依赖已有 BuildKit cargo/npm cache（勿随意 `builder prune`）。

### 使用
```bash
EXPORT_FAST=1 ./export-images.sh   # Mac 提速（内存够时）
SKIP_BUILD=1 ./export-images.sh    # 已有镜像只打 tar
# 正式发版：CI 构建推送 → 服务器 pull（本机最省时间）
```

---

## 2026-07-21 — Mac 导出修复：`no such service: 1` + OOM 加固

### 根因
- `docker compose build --parallel 1` 在 Compose v5 无效：`--parallel` 不是 build 选项，`1` 被当成服务名 → `no such service: 1`。
- 另：Mac Desktop ~8GB 时 `JOBS=2` 易 OOM（与架构无关）。

### 改动
- 导出/推送改为显式串行：`docker compose build backend` → `frontend`（Mac/Windows 一致）。
- Mac 默认 `CARGO_BUILD_JOBS=1`；Windows 默认 2；交叉架构强制 1 + 关 Cargo cache。
- Dockerfile / compose 安全默认 jobs=1。

### 使用
```bash
./export-images.sh                    # Mac：选 linux/arm64；JOBS=1；串行构建
CARGO_BUILD_JOBS=2 ./export-images.sh # Desktop 内存 ≥12GB 时可试
.\export-images.ps1                   # Windows 默认 JOBS=2；OOM 时 $env:CARGO_BUILD_JOBS=1
```

---

## 2026-07-20 — Docker 镜像构建降内存 / 提速（Mac·Windows·Linux）

### 根因
- `codegen-units=1` 推高 LLVM 峰值，Docker Desktop 易 OOM；无 BuildKit cache 导致反复全量编译。
- `Cargo.lock` 曾被 dockerignore 误排除；导出脚本 Darwin 分支缺 `fi`。
- 复查补修：跨架构 cache 未按 `TARGETARCH` 隔离（amd64/arm64 可能串缓存）；`set -e` 下无效的 `$?` 检查。

### 改动
- `backend/Dockerfile`：按架构隔离 BuildKit cache、`CARGO_BUILD_JOBS`；运行时仍 debian bullseye。
- `frontend/Dockerfile`：`npm ci` + npm cache；nginx 不变。
- `Cargo.toml` release：`opt-level=3`、`codegen-units=16`。
- 导出/推送脚本内联 BuildKit、平台与 jobs；交叉架构强制 jobs=1。

### 使用
```bash
./export-images.sh
DOCKER_DEFAULT_PLATFORM=linux/arm64 ./export-images.sh
.\export-images.ps1
```
勿用 Mac 本机二进制替代镜像内程序；导入与 `docker compose up -d` 不变。

---

## 2026-07-20 — 系统概览性能优化

- 实时吞吐轮询 2s → 5s；`/metrics/live` 鉴权仅校验 JWT，跳过 `is_active` 查库
- 去掉日期快捷「全部」；RangePicker 不可清空（避免无界全表聚合）
- `dashboard_cache` 后台每 5 分钟清理超过 30 分钟的缓存条目

## 2026-07-20 — 系统概览仪表盘布局精简

- 请求数与总令牌合并为一张卡片；QPS / RPM / TPM / Task 合并为一张「实时吞吐」卡片
- 消耗 Token、预估成本卡片保持不变；去掉原先独立的 4 格实时吞吐行

## 2026-07-20 — 系统概览默认展示今日数据

- 控制台「系统概览」打开时，日期快捷选项与区间默认从「本月」改为「今日」。

## 2026-07-20 — Dashboard 实时吞吐观测（QPS/RPM/TPM/Task）

- **能力**：系统概览页顶部新增实时吞吐条（QPS / RPM / TPM / Task）；Admin 看全局，普通用户看本人所有 API Key 汇总
- **后端**：`middleware/live_metrics.rs`（static 原子全局 + DashMap 按 token 分槽 + 双 RAII Guard）；埋点在 `api_key_middleware`、`record_and_bill_inner` 与异步 `execute_settlement_tx`；`GET /api/v1/metrics/live`；冷用户 1h TTL / 5min 清理
- **约束**：P0 仅观测不限流；热路径无 Mutex；不写库
- **前端**：`Dashboard.tsx` 独立实时条，2s 轮询，页面隐藏时暂停

---

## 2026-07-20 — dev 启动支持后台 / 前台日志双模式

### 改动
- `dev.sh` / `dev.ps1`：默认 **后台**；`fg` / `DEV_ATTACH=1` 为 **前台日志**（Ctrl+C 仅停本实例）。
- 精简：去掉 `DEV_FAST` 预起二进制分支，统一 `cargo watch -x run`；复用 `port_in_use` / `follow_log`。
- 用法：`./dev.sh [1|2] [bg|fg]`、`.\dev.ps1 [1|2] [bg|fg]`。

### 使用
```bash
./dev.sh                 # 后台（默认）
./dev.sh fg              # 前台日志
.\dev.ps1                # 后台（默认）
.\dev.ps1 1 fg           # 前台日志
```

---

## 2026-07-20 — 加快本地开发启动（兼容多实例 / Windows）

### 改动
- `backend/Cargo.toml`：`[profile.dev]` 使用 `debug = "line-tables-only"`，缩短链接耗时（不影响 release / 业务逻辑）。
- `dev.sh` / `dev.ps1`：统一 `cargo watch -x run`；去掉易出端口冲突的「预起二进制 + postpone」快启分支，增量交给 Cargo。
- 多实例：仍按路径哈希隔离 state、端口避让、只回收本仓库进程；各 checkout 默认各自 `target`（Windows 非 ASCII 路径仍重定向到 `%LOCALAPPDATA%`）。

---

## 2026-07-20 — 对齐半开日期边界与前端绝对时刻传参

### 改动
- 后端：`parse_instant_bound` / `parse_timestamptz_bind` / `push_timestamptz_bound`，纯日期与无偏移时刻按 timedisplay 半开；终点 `< ?::timestamptz`。
- 修复：无偏移 `YYYY-MM-DD HH:mm:ss` 不再被误当成纯日期截断到 00:00。
- 前端：`dateRangeParams.ts`；管理端 usage-stats 同步也改传 ISO。
- 精简：去掉未用 `AbsoluteRange` / coarse 二元组 / `sql_timezone_convert` / 废弃上海边界函数；删除 `date_helper` / `time_system` 内单元测试样例；前端日期传参统一走 `dateRangeParams`（含 AdvancedMarketing / Settings）。
- 修复：日志详情 `get_log_detail` 对非管理员补齐级联脱敏（列表大字段本为 NULL，原先 sanitize 无效）；`dateRangeParams` 拦截 Invalid Date；去掉未接线的 `time_system/package` 与死代码。

---

## 2026-07-20 — 恢复 dev.sh 多实例兼容

### 改动
- `dev.sh`：保留「后台拉起、就绪退出」体验；按目录名设 `PROJECT_NAME`；复用已运行 Postgres；前后端端口占用时顺延；仅清理本仓库残留进程，不误杀其它目录实例。
- 等待就绪改为最长 600s，并每 15s 打印编译进度，避免首次重编译被误判为卡死。

### 使用
```bash
./dev.sh          # 本地后台（默认）
./dev.sh 2        # Docker 全容器
# 可选：PROJECT_NAME / BACKEND_PORT / FRONTEND_PORT / POSTGRES_PORT / DEV_WAIT_MAX
```

---

## 2026-07-20 — 创作中心时间存储与展示全量核对

### 结论
创作中心库表时间列已在 `timestamptz_unify_v1` 覆盖（`playground_projects` / `playground_assets` / `user_model_configs`）；API 读写用 `DbTs` + `NOW()` / `?::timestamptz`。本次补齐前端展示与画布 JSON 时间一致性，以及清理任务的时间解析。

### 后端
- `list_projects` / `get_project` / assets / `user_model_configs`：`created_at`/`updated_at` 用 `DbTs` 解码（避免 TIMESTAMPTZ→String 崩溃）
- `cleanup_stale_playground_nodes`：画布 `created_at` 用 `parse_flexible_ts`；日志匹配绑定 `DbTs` + `?::timestamptz`

### 前端
- 统一经 `parseApiTimeAsUtc` / `formatApiDateTime`（timedisplay）：项目列表、悬浮头、创作日志、资源管理、节点详情、Token 弹窗
- 画布 `taskData.completed_at` 由毫秒时间戳改为 ISO 字符串，与 `created_at` 一致
- 资产回填/超时判定按 UTC 解析，不再依赖浏览器本地 `new Date(无偏移字符串)`

### 部署
前后端同步发布；后端需重启。无新迁移。

---

## 2026-07-20 — 复查：补修 playground TIMESTAMPTZ 解码为 String

### 问题
日志：`decoding column 7: String not compatible with TIMESTAMPTZ`。
创作中心项目列表 SQL 第 7 列是 `created_at`，却用元组 `String` 解码。

### 改动
`playground.rs`：`list_projects` / `get_project` / assets / `user_model_configs` 改为 `DbTs`。

### 部署
重启后端生效。

---

## 2026-07-20 — README 精简与约定补齐

### 改动
- 重写根目录 `README.md`：去掉重复营销与过时章节，保留部署 / 开发 / 运维要点
- 修正管理员默认密码表述不一致；补齐金额 6 位小数、TIMESTAMPTZ、日志归档约定
- 变更历史仍以本文件为准，README 仅作入口链接

### 同步小修
- 渠道配置额度展示、公告低余额阈值输入改为 6 位小数
- `money::format_money` 供通知等格式化复用，消除未使用常量告警

---

## 2026-07-20 — 金额精度统一为小数点后 6 位

### 约定
站点内部账本（日志 cost、扣费结算、余额/赠送金/信控、充值调账、额度用量）一律保留 **6 位小数**（四舍五入）。
支付通道对外法币金额（微信/支付宝等）仍按通道要求保留 2 位，不在此范围。

### 改动
- 后端新增 `money::round_money`，接入余额 API、管理员充值、计费结算、预扣拆分、额度微单位
- 前端日志/财务/钱包/用户/令牌/渠道/仪表盘等金额展示统一 `toFixed(6)` / `precision={6}`
- 前端金额展示统一 `toFixed(6)` / `precision={6}`（与后端 `money::round_money` 对齐）

### 部署
前后端同步发布；无新库迁移。

---

## 2026-07-20 — 安全加固：代登鉴权 / OAuth State / 验证码防爆破

### 问题（安全检测 P0）
1. 代登接口 handler 未显式校验管理员 Claims（虽路由层有 middleware，缺少防御纵深）
2. OAuth state 存在过松兼容路径：任意 `wechat_XXXXX` 可通过校验，绕过 HMAC
3. 邮箱/短信验证码无尝试次数限制，6 位数字可暴力破解

### 改动
- `impersonate_user`：强制 `role == admin`，并记录审计日志
- 删除 OAuth state 前缀兼容分支；仅接受服务端 HMAC 签发；登录页/注册页改为请求 `/auth/oauth/state`
- 绑定/换绑微信与谷歌 state 同样改为 HMAC 签发（`/user/bind/oauth-state`）
- `verification_codes` 新增 `attempts` 列；错误超 3 次作废；有效期改为 5 分钟

### 部署
重启后端以执行迁移 `verification_codes_attempts_v1`；前端需同步发布。

---

## 2026-07-20 — 修复 TIMESTAMPTZ 与 TEXT 比较导致 Internal database error

### 问题
列改为 `TIMESTAMPTZ` 后，部分接口仍用字符串参数做 `created_at >= ?`，PostgreSQL 报错：
`operator does not exist: timestamp with time zone >= text`。
前端表现为：日志/任务列表有时能出数据，同时弹出 **Internal database error**（并行 COUNT/列表或其它带时间筛选的接口失败）。

### 改动
所有对 `TIMESTAMPTZ` 列的范围比较统一为 `?::timestamptz`（含 logs 已有路径、dashboard / finance / auth / user wallet / team_marketing / happyhorse / `date_helper::sql_cond`）。

### 部署
重启后端使新二进制生效即可（无新迁移）。

---

## 2026-07-19 — 时间体系统一：线上自检 / 财务用户展示 / logs 归档

### ① 线上自检 SQL（TIMESTAMPTZ）

在业务库执行，确认迁移 `timestamptz_unify_v1` 已落地：

```sql
-- 1) 迁移是否执行
SELECT id, executed_at FROM sys_migration_history
WHERE id IN ('timestamptz_unify_v1', 'logs_archive_v1')
ORDER BY id;

-- 2) 关键表时间列类型（期望 timestamp with time zone）
SELECT table_name, column_name, data_type
FROM information_schema.columns
WHERE table_schema = 'public'
  AND (
    (table_name, column_name) IN (
      ('logs', 'created_at'),
      ('users', 'created_at'),
      ('users', 'updated_at'),
      ('orders', 'created_at'),
      ('orders', 'paid_at'),
      ('recharge_records', 'created_at'),
      ('commissions', 'created_at'),
      ('verification_codes', 'expires_at')
    )
  )
ORDER BY table_name, column_name;

-- 3) 仍为 text/varchar 的业务时间列（理想应为空；周期键 last_reset_* 除外）
SELECT table_name, column_name, data_type
FROM information_schema.columns
WHERE table_schema = 'public'
  AND column_name ~ '(created_at|updated_at|paid_at|expires_at|last_used_at|expire_at)$'
  AND data_type IN ('text', 'character varying')
  AND column_name NOT LIKE 'last_reset%'
ORDER BY table_name, column_name;

-- 4) logs 热表体量与索引
SELECT relname, n_live_tup, n_dead_tup
FROM pg_stat_user_tables WHERE relname IN ('logs', 'logs_archive');

SELECT indexname FROM pg_indexes
WHERE tablename = 'logs' AND indexname LIKE '%created%';
```

### ② 财务 / 用户页展示

- `formatApiDateTime` 改为按 **timedisplay**（用户时区 > 站点默认）直接格式化，不再依赖浏览器本地时区。
- 已切换：`GiftRecords` / `RechargeRecords` / `Users` / `AdminGroups` / `UserLevels`（订单详情此前已用）。

### ③ logs 分区 / 归档方案

**已落地（Phase 1 — 冷表归档）**

| 项 | 说明 |
|---|---|
| 表 | `logs_archive`（`LIKE logs` + `archived_at`） |
| 开关 | `storage_settings.log_row_retention_days`（默认 **0**=不归档） |
| 时机 | 每天凌晨详情清理后，分批每批 5000 行：先 INSERT 冷表再 DELETE 热表 |
| 缓冲 | 实际阈值 = 配置天数 **+2**，降低统计未落档风险 |
| 建议 | 大数据量站点：先校准 `usage_daily_stats`，再设 `90`；且 ≥ `log_retention_days` |

**Phase 2 — 原生月分区（运维窗口，按需）**

热表仍过大时，在维护窗口把 `logs` 改为 `PARTITION BY RANGE (created_at)`。要点：

1. 新建分区父表 + 按月子表（含未来 2～3 个月）。
2. `INSERT INTO logs_partitioned SELECT * FROM logs`（或按月批次）。
3. 交换表名 / 重建索引 / 切应用；旧表改名备份后再 DROP。
4. 冷表 `logs_archive` 可同样按月分区，或按年 DETACH 后迁对象存储。

> 未自动执行 Phase 2：线上改分区需短时锁表与校验，请单独排期。

### 部署

停旧后端 → 启新二进制跑迁移（含 `logs_archive_v1`）→ 管理端按需设置「日志行归档天数」→ 再开流量。

---

## 2026-07-19 — 重置密码/注册：获取验证码防刷

### 问题
重置密码页「获取验证码」可连点，`sendingCode` 为异步 state 拦不住并发请求，失败时也不开倒计时，导致错误弹窗刷屏。

### 改动
- `ForgotPassword` / `Register`：`useRef` 同步锁 + `cooldownUntilRef` 时间戳冷却，堵住 state 提交竞态。
- 成功冷却 60s；失败短冷却 3s；切换找回方式不再清零倒计时。
- 请求中禁用按钮并显示 loading。
- 后端已有 `check_code_send_cooldown`（60s），前端防刷主要解决弹窗刷屏。

### 涉及文件
- `frontend/src/pages/Login/ForgotPassword.tsx`
- `frontend/src/pages/Login/Register.tsx`

## 2026-07-19 — 站点时间体系统一（落库 / 查询 / 展示）

### 改动
- `once_migration!`：任一句失败不写 history，支持重启重试。
- TIMESTAMPTZ 写入统一为 `DbTs::now()` / `CURRENT_TIMESTAMP`（去掉 `::text`、朴素字符串绑定时戳列）。
- 注册邀请日限额 / IP 日限额：`created_at LIKE` 改为站点 timedisplay 自然日 `[start, end)` 范围查询。
- 验证码校验：用 `expires_at > NOW()`，不再做字符串字典序比较。
- 前端 `timedisplay.ts` 导出 `formatApiDateTime` / `parseApiTimeAsUtc`；日志、订单、快乐小马等展示统一走该函数。

### 部署
停旧后端 → 启新二进制跑迁移 → 再开流量；勿与旧二进制混跑。

## 2026-07-19 — 全库时间列 TEXT → TIMESTAMPTZ

### 问题
绝大多数 `created_at`/`updated_at` 等以 TEXT 存储，logs 等热路径频繁 `::timestamptz` 转换，btree 索引难以有效服务时间范围查询，日志性能已顶不住。

### 改动
- 新增 `DbTs`（`TIMESTAMPTZ` ↔ API RFC3339 字符串），FromRow 模型时间字段统一改用该类型。
- 一次性迁移 `timestamptz_unify_v1`：业务时间列改为 `TIMESTAMPTZ`（周期键 `last_reset_*` 仍为 TEXT）。
- logs/dashboard/清理/归档等查询去掉列上 cast，改为对参数 `?::timestamptz`，便于走索引。
- 运行时写入将 `now()::text` 改为 `NOW()`。

### 部署注意
`logs` 大表 `ALTER TYPE` 会重写表并短时锁表，请安排维护窗口后重启后端以执行迁移。

### 涉及文件
- `backend/src/time_system/db_ts.rs`（新增）
- `backend/src/db/migrations.rs`
- `backend/src/models/*`、`backend/src/api/logs.rs`、`dashboard.rs`、`date_helper.rs` 等

## 2026-07-19 — 系统概览：日期口径对齐与范围标签

### 问题
管理端/用户端共用仪表盘，但「总*」大数字随筛选变化，今日/昨日副行与模型「近三天」却固定日历日，默认又落在「今天」，易被误判为数据错误。

### 改动
- 默认筛选改为「本月」，并恢复「全部」；有筛选时主指标文案跟随上方快捷标签（如「本月请求数」），今日/昨日副行仅在「今天/昨日」快捷项下显示。
- 标题旁标明数据范围：管理员「全站」/ 用户「仅本人」；管理端最近活动增加用户列。
- 模型明细近几日改为锚定筛选区间末日（≤ 今天）向内最多 3 天；「全部」仍为日历近 3 天。
- 最近活动查询关联 users，填充昵称/UID。

### 涉及文件
- `frontend/src/pages/Dashboard/Dashboard.tsx`
- `frontend/src/locales/zh.json` / `en.json`
- `backend/src/api/dashboard.rs`
- `backend/src/api/date_helper.rs`

## 2026-07-18 — 日志记录 / 任务列表：防连点与大数据量性能优化

### 问题
查询 / 重置 / 刷新在数据未返回时被疯狂点击，会叠加大量请求，浏览器与后端易被打崩；列表接口还把请求/响应大字段整包返回，数据量一大就更慢。

### 改动
- **前端**：新增 `QueryGuard`（新请求取消旧请求 + AbortController）；查询/重置/刷新按钮 loading 时禁用；取消中的请求不报错。
- **后端列表瘦身**：`/logs`、`/task_logs` 列表不再返回 `request_content` / `response_content` / `post_response` / `upstream_req_content`。
- **按需详情**：新增 `GET /logs/{id}/detail`；表格展开行与任务预览时再拉取大字段。
- **并行查询**：列表 COUNT / 数据 /（日志）汇总改为 `tokio::join!` 并行执行。

### 修复（同日）
- 去掉 1.2s 时间节流与“操作过于频繁”提示（StrictMode 重挂载会误伤首屏）。
- 全局 axios 拦截器忽略主动取消的请求，避免误报 `Network error`。

### 涉及文件
- `frontend/src/utils/queryGuard.ts`
- `frontend/src/utils/request.ts`
- `frontend/src/pages/Logs/Logs.tsx`
- `frontend/src/pages/Logs/TaskLogs.tsx`
- `backend/src/api/logs.rs`
- `backend/src/api/task_logs.rs`
- `backend/src/api/mod.rs`
- `backend/src/models/log.rs`
