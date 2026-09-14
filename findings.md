# 设计依据

## 0.5.1 用户界面反馈

- Advanced 原先直接枚举完整文档，所以 DNS 等已有专页对象重复出现；底层只有一份配置，但重复入口造成心智混乱。本次仅从 Advanced 列表排除 dns/route/inbounds/outbounds，完整 JSON 入口仍保留，未删除任何原生字段。
- 用户偏好旧版顶部导航，取消侧栏；采用顶部自适应换行标签、1–9/0/- 直达和 Tab+左右导航。列表始终占用主体宽度，窄终端不再切换成侧栏菜单。
- 主要操作不能只藏在问号帮助：页内展示节点订阅导入、g 建组、C 导入 QX/Clash 规则以及原生规则入口差异；成员选择说明返回表单后仍需保存，导入 F2 标注为审阅而非直接保存。
- 修改只涉及界面/快捷操作入口，原生配置模型、管理协议 6 和运行状态不变；0.5.0 用户退出并重开界面即可，不需要断开核心、再迁移或重新导入订阅。

## 0.5.0 实现结论（覆盖下文旧版描述）

- 配置权威从旧 Settings/Rule 模板生成改为 Store.native；旧模型仅保留迁移与资源来源元数据。表单的原值直接取自私有文档，只提交实际修改字段，展示快照的脱敏占位符绝不回写配置。严格 JSON 保留语义，不保留注释/空白。
- Rule/Global/Direct 是临时流量路由覆盖，不是内核配置的全部框架；保留 DNS 和所有拨号依赖，因此 Direct 不能承诺内部 DNS 完全不使用代理。这一影响在应用审阅中明确展示。
- 原生管理 API 依赖受保护的 management 服务（本机回环地址、端口、secret、非 TLS）。其他 endpoints/services/TLS/transport 等字段可编辑并交由所选核心校验；没有声称所有原生能力已有专用表单。
- DNS 直连是留空 detour；空 direct 出站作为 DNS detour 会触发内核启动问题，现可提前拦截。默认节点域名解析、DNS 服务器 bootstrap 与 DNS 查询路由为不同配置对象；跨出站/DNS 依赖检查可发现循环。
- 节点导入在首个空默认 selector 中填入成员一次；以后已有组成员是显式配置，源节点移除后须修复引用。局部编辑与订阅更新冲突时失败保留草稿。原生远程规则和客户端转换规则分开更新；转换规则不隐式重建 DNS 或既有路由次序。
- 校验依据为本地 sing-box 1.14.0 和固定版本文档：https://raw.githubusercontent.com/SagerNet/sing-box/v1.14.0/docs/configuration/dns/server/https.md 、https://raw.githubusercontent.com/SagerNet/sing-box/v1.14.0/docs/configuration/dns/server/udp.md 、https://raw.githubusercontent.com/SagerNet/sing-box/v1.14.0/docs/configuration/service/api.md 。本地真实核心验证 local/UDP/TCP/TLS/HTTPS/QUIC/H3/FakeIP 配置；只验证格式及启动路径，不将配置校验当作外部 DNS 可达性或性能测量。
- 新版仍有明确产品边界：实际 TUN 网络恢复与 Linux/SSH 验收未完成，进程条件可编辑但未提供 App 自动选择器，订阅调度/动态成员策略/完整原生配置导入向导/性能诊断/服务自启仍需后续。完整能力入口与专用易用工作流是两个不同完成度。

## 2026-09-15 架构反思：原生语义优先（待确认方案）

- 用户新决定：后续只保留英文 UI，非必要常驻提示应减少；此项取代下文早期中英文双语偏好。要求先讨论整体方向，再 Git 备份，再升级。
- 现状证据：src/model.rs 的 Rule/MatchRule 为单 kind/value，组 members 只允许 node IDs；src/config.rs 固定 sniff/DNS/private-IP/手工规则/绑定规则顺序，入站固定 mixed 与可选 TUN，非 Rule 模式删去部分有效对象并强制 paired DNS。src/ui.rs 的八页混合配置资源和运行观察；Settings 与高级表单重复入口。节点原生 outbound 已保留 JSON，但整个原生配置尚不能保真往返。
- 推荐配置对象层（Inbounds、Outbounds、Routing、DNS、Resources、按需高级对象）与运行观察层（Connections、Logs、Diagnostics）分开；节点/组是 Outbounds 的不同类型，不建立两份编辑状态。资源管理保留节点订阅与规则集两个分类；Rules 只负责有序匹配与动作。
- 推荐原生配置文档作为配置事实来源，客户端元数据单独保存；表单局部编辑保留未知字段及复合结构，未覆盖能力可用原生编辑入口；支持范围取决于所选内核版本、编译能力和平台，不能把原生文档中的移动端/图形客户端功能承诺给 CLI。
- 保留 Rule/Global/Direct 的快捷用途，但作为明确、可预览/撤销的客户端覆盖，不把三个模式作为整个配置模型；DNS 与依赖对象不能再被隐式重写/删除。复杂原生配置存在冲突时应阻止自动覆盖或要求明确选择，而非静默简化。
- 原生规则集应保留 inline/local/remote 与 source/binary 形式，不能经过 QX/Clash 简化转换器；跨格式转换是独立导入辅助。下载路径、缓存、更新和首次启动依赖需要设计；原生远程资源与客户端转换资源各自有明确更新所有者。
- 补充范围：多入站/绑定与鉴权、TUN 路由/DNS/双栈/MTU、嵌套 selector/urltest/拨号依赖、完整有序路由与逻辑条件/动作、独立 DNS、原生 Endpoints 与高级 TLS/传输等保真入口。后续优先级由普通代理客户端路径决定，不扩展成全功能服务器运维平台。
- DNS 更正（覆盖早期笼统描述）：1.14 TUN dns_mode 默认 hijack，包括可用平台的原生接口 DNS 设置；生成器未显式配置该值，所以旧帮助中的“no system DNS takeover”不能用于 TUN。尚未修改帮助/README或做真实 TUN 验收。
- 官方核实： https://raw.githubusercontent.com/SagerNet/sing-box/v1.14.0/docs/configuration/route/rule.md 、https://sing-box.sagernet.org/configuration/route/rule_action/ 、https://raw.githubusercontent.com/SagerNet/sing-box/v1.14.0/docs/configuration/rule-set/index.md 、https://raw.githubusercontent.com/SagerNet/sing-box/v1.14.0/docs/configuration/dns/index.md 、https://raw.githubusercontent.com/SagerNet/sing-box/v1.14.0/docs/configuration/inbound/tun.md 、https://sing-box.sagernet.org/configuration/shared/dial/ 。网站当前页面可能超前于 1.14；实现前逐项核对所选版本，弃用字段不能照示例盲用。
- 交互参考重新查阅 LazyGit 官方仓库和 K9s commands：列表/详情、局部操作、搜索/上下文帮助；借鉴交互机制而不是照搬多面板密度。
- 工作区目前没有 .git；已有 .gitignore 和 0.4.1 release 可执行文件。未来 Git 仅跟踪审计后的源码/文档/虚构 fixtures；运行数据、订阅凭据、日志、构建产物不入库。Git 源码基线不等于运行数据备份，后者应私有独立且不复制活动 socket。

## 0.4.1 实际启动故障与界面语义

- 只读检查用户应用数据中的非凭据字段：capture=tun、route_mode=rule、routing=direct、dns_policy=legacy，已启用一个分类规则绑定。core.log 多次 FATAL 均为 `start dns/https[secure-dns]: detour to an empty direct outbound makes no sense`。未读取/复制完整节点凭据，未切换或停止真实核心。
- 原因是旧单解析器配置始终写 detour=s.routing；direct 空出口不能作为 DNS dialer detour。修复为 routing=direct 时省略 detour，保留 HTTPS 解析器和地址，而不是偷偷迁移 DNS 策略。参考官方 Dial Fields：https://sing-box.sagernet.org/configuration/shared/dial/ 。单测先复现失败，再修复；真实 core 的 legacy/direct 规则模式启动与本地分流回归通过。
- 原 Home Proxy choice 在 Rule 下显示无关默认节点，Global 命名组只显示组名；改成模式目标+实际组成员（原生接口），未应用设置不作为 LIVE。取不到实时组时标未知，不留上次组快照冒充当前。
- 设置选项存储键与显示名称分开，中英文只影响显示，不改 group ID/JSON 值。诊断使用界面已保存语言，避免运行配置的旧语言干扰切换。

## 0.4 原生路由模式与连接观察

- 模式独立于 capture（port/system/tun），由客户端生成原生路由/DNS配置，不依赖 Clash API 或改动 default_mode。global 绕过用户规则并选择全局组；direct 不加载代理节点和自动测速组；保留用户原规则，切回 rule 恢复。
- 官方 v1.14 协议与实现核对：https://github.com/SagerNet/sing-box/blob/v1.14.0/daemon/started_service.proto 与同目录 started_service.go。SubscribeConnections 初始 reset 同时包含当前和近期关闭记录（都是 NEW 事件），由 closedAt 区分；只取初始快照可避免后台保留用户完整浏览历史。连接界面每约2秒采样、最多500条，显示过期和失败。
- 真核心验证发现普通 HTTP 代理请求在响应完成时可能立即结束跟踪，不能把 keep-alive 客户端 socket 当成同一持续核心连接。改用 CONNECT 隧道后验证：准确来源端口/出口/入站、关闭指定 UUID、另一隧道继续正常返回响应。测试不把超时当成关闭成功。
- DNS 模式覆盖经真实核心配置校验，但未测真实 DoH 耗时、解析泄漏或带宽；D 为本地配置解释而非健康保证。系统代理覆盖、TUN/SSH风险、进程未知状态均需明确展示。
- 配套检查发现旧回滚只恢复节点与设置，未恢复分组/规则资源/绑定；本轮补齐同一快照的策略恢复，防止路由模式回滚留下不一致引用。

## 具体 YouTube 远程规则样本

- 用户澄清只需要分类规则订阅，并提供 https://raw.githubusercontent.com/blackmatrix7/ios_rule_script/master/rule/QuantumultX/YouTube/YouTube.list 。
- 本次读取内容与文件头一致：196 条，HOST 3、HOST-SUFFIX 179、HOST-KEYWORD 1、HOST-WILDCARD 3、IP-CIDR 2、IP6-CIDR 1、USER-AGENT 7。文件标注更新时间 2025-06-06；不是本次测速或已转换结果。
- 186 条常见域名/IP 匹配存在原生对应字段；3 条 wildcard 需核实 QX 匹配语义后转换成受约束的 domain_regex；7 条 USER-AGENT 不具备普通 sing-box 路由/headless rule 等价字段，不能冒充进程规则或随意按域名替代。未实际生成/验证转换文件，不能声称 189 条已通过。
- 末尾 YouTube 是原策略字段；规则订阅源、内容集合、用户策略组和有序绑定应独立。保留源策略用于审计，用户显式指定本机目标，不自动创建同名组或覆盖用户策略。
- 跨格式转换应只在导入/更新时发生，归一化后生成原生 JSON/SRS 给核心匹配；不是运行时增加 Clash/QX 转发层。
- 规范化建议：源保留 URL/格式/版本/更新时间，集合负责识别，组负责出口选择，有序路由负责绑定，DNS 协调解析；更新失败保留上次有效内容，显著语义变化需确认。规则计数不代表服务覆盖率或性能保证。

## 分组与规则产品建议（待用户决定实施）

- 原生分组基础为 selector / urltest；第一阶段建议用户命名的手选组和自动延迟组，成员引用原节点，不复制凭据；自动最低延迟不等于视频吞吐或地区解锁最优。
- 网站规则集合与执行策略分离：规则集存匹配条件，路由把它关联到分组 / direct / reject。一个集合可复用；不得把外部完整配置的混合动作无提示压成同一目标。
- 跨格式初期支持 sing-box JSON/SRS（验证核心兼容）、Clash/Mihomo 常见文本/YAML domain/ipcidr/classical 子集、QX filter 常见 host/host-suffix/host-keyword/ip-cidr。MRS、GEOIP/GEOSITE 依赖、no-resolve、复杂逻辑等需单独适配；不通过改后缀冒充转换，不执行导入脚本，不静默忽略不支持项。
- QX filter.snippet 包含 direct、proxy、geoip、final，sample.conf 明确 force-policy 会覆盖原动作。因此 UI 需分“绑定集合统一策略”和“保留原顺序并映射策略”，final/match 等全局兜底需明确确认。
- 进程匹配原生支持 macOS/Linux 的 process_name/path/path_regex；App bundle 与联网 helper 进程不一一对应，实际识别和权限需验证。前提是流量已进入 sing-box；system 不保证覆盖不遵循代理的 App，TUN 用于更广接管但不承诺无遗漏，SSH 仅作用远端。
- DNS 建议与路由共同生成：引导、直连、代理解析分离；明确缓存、IPv6、系统/浏览器 DoH 的覆盖范围。不能机械复制 IP/进程规则到 DNS，无法识别时需透明显示。FakeIP 不是第一版提速开关。
- 必须补的产品保护：规则优先级和最终兜底、组为空/失效成员、默认不自动退直连、更新缓存/预览/失败保旧/回滚、规则下载引导、诊断显示实际命中与出口（无元数据时明确不可判定）。
- 官方参考：https://sing-box.sagernet.org/configuration/outbound/urltest/ 、https://sing-box.sagernet.org/configuration/route/rule/ 、https://sing-box.sagernet.org/configuration/rule-set/ 、https://sing-box.sagernet.org/configuration/dns/rule/ 、https://wiki.metacubex.one/en/config/rule-providers/content/ 、https://raw.githubusercontent.com/crossutility/Quantumult-X/master/filter.snippet 、https://raw.githubusercontent.com/crossutility/Quantumult-X/master/sample.conf 。

## 0.2 试用后的只读诊断

- 用户补充 QX 对比为同机同节点、首开和视频均慢，优先排查生成配置与接管路径差异，不直接归因于节点或 DNS。
- 已有 DNS 配置：系统 local 作 bootstrap，默认 HTTPS 1.1.1.1 随默认路由 detour，dns.strategy=ipv4_only，未禁用缓存；目前缺少直连/代理 DNS 分流及解析诊断。system 模式只改代理设置，不接管全机 DNS。
- 官方 DNS 文档明确 disable_cache 默认为 false，1.14 optimistic caching 是独立可选能力；不能因为 UI 没有缓存开关就声称现在没有 DNS 缓存。https://sing-box.sagernet.org/configuration/dns/

- 用户已实际授权并成功代理上网，但报告 System takeover NOT verified 和相较 QX 速度慢。
- 当前读取时 manager_protocol=2、connected=false、configured=false、effective=false、pending_restore=false，助手明确返回原设置已恢复。活动最后为用户断开，而非自动恢复或崩溃。macOS 有效代理只有 FTPPassive=1。
- 已保存 system / proxy / DoH 1.1.1.1，用户规则数为 0，当前所选节点类型 VLESS + TLS。未读取或输出订阅 token、节点凭据或服务器地址。
- UI 在核心未运行时采用已保存模式，仍渲染 System takeover NOT verified，混淆“未启用”和“启用但无法验证”。连接时该提示也可能表示 configured / effective 任一未满足，不能凭历史提示判定误报。
- 默认策略只有私有 IP 直连，没有国内外规则集；不能假设 QX 使用相同路由或节点。无同条件测速前不能归因于内核。
- 路由无条件 sniff，官方文档默认 timeout 为 300ms（不是每次固定延迟）；只列作首开等待的排查项，尚无测量证明其造成当前慢。https://sing-box.sagernet.org/configuration/route/rule_action/#timeout

## 用户意图

面向 macOS 和 Linux 终端，提供订阅导入、节点选择以及辅助编写 sing-box 配置的一体化体验。用户不需要掌握订阅格式和 JSON 配置结构。初始为设计讨论，现已授权实现客户端与 macOS 系统代理功能；本轮不实际启用用户主机系统代理或 TUN，特权写入验收另行确认。

## 已检查的旧项目

https://github.com/mmei0114/fly-auto-singbox

已检查 scripts/extract_nodes.py、internal_subscribe/tool.py、parsers/clash2base64.py、parsers/vless.py、README 和订阅模板。订阅通过 HTTP 下载，在本地识别 URI 列表、Base64、Clash proxies 和 sing-box outbounds。Clash 节点经过 URI 中间格式转换；整个导入流程无需运行 Clash，也不调用在线转换服务。提取流程强制筛选四地区，部分解析异常被跳过。新项目参考其输入覆盖，不继承限制与旧代码的兼容性假设。

## 交互参考（2026-09-13 查阅）

- LazyGit 官方快捷键：https://github.com/jesseduffield/lazygit/blob/master/docs/keybindings/Keybindings_en.md
  借鉴列表导航、焦点操作、Esc 退出当前上下文、就地帮助、可查看变更。
- K9s 官方命令：https://k9scli.io/topics/commands/
  借鉴资源筛选、可搜索操作、详情深入和上下文帮助；不要求新用户记冒号命令。
- btop 官方仓库：https://github.com/aristocratos/btop
  借鉴实时网络图表、可点击的快捷键标签与设置菜单。
- Television 官方仓库：https://github.com/alexpasmantier/television
  借鉴快速筛选与预览，用于节点和操作搜索。

这些是交互机制参考，不复制品牌、完整页面或特定视觉主题。

## API 边界

- 官方管理 API 文档：https://sing-box.sagernet.org/configuration/service/api/
- 官方版本说明：https://github.com/SagerNet/sing-box/releases/tag/v1.14.0
- 官方 Remote Profile 定义：https://sing-box.sagernet.org/clients/general/

管理 API 的角色是运行状态和控制。订阅获取、配置生成、持久化、初次启动和系统集成仍需客户端实现。管理 gRPC 与节点协议的 gRPC transport 无关。官方接口的具体版本、方法和连接统计依赖须在实现前以选定版本源码及集成测试核实。不可直接承诺所有管理功能都不需要 sing-box 内部 Clash 兼容组件。

## 用户已确认与后续事项

- 第一版覆盖 Mac 本机 + Linux 本机及 SSH，为无桌面环境提供完整核心操作。
- 英文优先，保留中文切换；设计讨论文档采用中文，界面线框采用英文。
- 产品正式名称为 sing；分发和签名方案尚未确定。

## 系统代理实现依据与边界（2026-09-14）

- 通过 Apple 官方 SystemConfiguration 文档及本机 SDK Headers 核实 SCPreferences、SCNetworkSet/Service、CoreFoundation property list 和 SCDynamicStoreCopyProxies。参考：https://developer.apple.com/documentation/systemconfiguration/scpreferences 、https://developer.apple.com/documentation/systemconfiguration/scdynamicstorecopyproxies(_:) 。
- 使用 SCPreferences 锁内比较、提交、应用；完整字典类型化备份，HTTP/HTTPS/SOCKS/PAC 组级比较恢复。有效状态单独读取，不把核心就绪等同于系统接管或互联网畅通。
- root helper 只处理固定应用实例的代理设置，root 私有记录目录；socket 在 root 拥有的目录中创建，避免在用户可替换路径下执行 chmod/chown。TUI 不接收密码。
- root helper 活着时可基于独立管理器心跳及端口健康尝试恢复；尚无 launchd，因此助手同时被杀或断电不保证即时自动恢复，下一次授权优先恢复持久记录。
- 系统代理不替代 TUN；绕过列表、其他网络位置和不遵循系统代理的程序都影响实际覆盖范围。当前 Linux 无自动桌面代理集成。
- 原生只读测试识别到 5 个启用的物理类网络服务，读取和 plist 往返验证通过，未打印其配置或修改它们。
# 实现核实补充

## 0.3 规则 / DNS 实现边界

- 官方 headless rule 的域名族与目的 IP 条件为 OR，进程条件与其他字段为 AND；原生复合进程规则不能拆成多个宽泛的 OR 条件。实现只接受可证明等价的简单子集。参考 https://sing-box.sagernet.org/configuration/rule-set/headless-rule/ 。
- 分类资源与目标分离：源文件策略只在预览显示，最终采用用户选定的 binding；缓存转换结果为 inline rule-set，核心无需在线下载分类规则。
- paired DNS 只投影域名匹配；不从 IP / process 猜 DNS 规则，原生 bootstrap 仍独立使用系统解析。此模式不接管系统 DNS，也不能保证覆盖应用自带 DoH 或远端解析。参考 https://sing-box.sagernet.org/configuration/dns/rule/ 。
- 自动 URLTest 的原生配置校验通过；为避免未授权公开测速，自动化不启动 urltest 组，只在手动组上验证 gRPC。自动组性能 / 网站解锁能力未验证。
- 新增规则管理使快照更大；更新 diff 已使用哈希集合，避免对大列表进行平方级比较。规则资源限制 8 MiB、5 万有效匹配，超限失败且保留旧值。

## 改名与连接问题诊断

- 用户已成功下载核心、导入节点并测速；本轮读到 28 个节点、核心版本 1.14.0、保存模式 port，但检查时核心已处于 disconnected。
- macOS `scutil --proxy` 有效配置仅 FTPPassive=1，没有启用 HTTP/HTTPS/SOCKS 系统代理。
- 因而先前“核心连接 / 测速成功”不意味着浏览器自动经过代理。由于诊断时核心已停止，本轮不能确认选中节点的端到端网页访问能力。
- 用户第二问是诊断请求，本轮不启动真实核心、不修改系统代理或启用 TUN。改名使用旧数据目录兼容，避免丢失订阅或重复管理器。

- 官方 1.14.0 已于 2026-08-31 发布；API Service 可作为 `services` 中 `type: api`，独立开启 gRPC，无需 Clash API。
- 真实核心验证了 GetVersion / SubscribeStatus / SubscribeGroups / SelectOutbound / SubscribeLog 的消息定义。
- GitHub release API 会限流；自动安装使用官方 release assets 页面核实的固定 SHA-256，不使用第三方镜像或转换服务。
- 当前机器 Homebrew 安装的是 1.13.2，需独立 1.14+ 核心；测试下载不会覆盖系统安装。
- 普通沙箱禁止 loopback 监听；相关测试经权限审核后只在本机回环地址运行，测试结束清理。
