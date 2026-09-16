# sing 产品交互研究：从配置编辑器到日常代理客户端

研究日期：2026-09-15。基于本地 0.5.1（b476cdb）。状态：设计建议，等待确认；不是已实现功能清单。

> 历史研究：本文的六页信息架构和独立原型建议已被 [正式客户端产品规格 v1](product-spec-v1.md) 取代。本文保留素材与推理，不再作为实施指令。

## 结论

保留原生 sing-box 配置作为唯一事实来源，但不要把它的 JSON 章节直接当成主导航。日常界面围绕用户任务组织，编辑器围绕原生对象组织，两者操作同一份数据。

建议产品定位：**可直接使用的代理客户端，同时具有可检查、可解释、可深入编辑的原生工作区。**

当前问题不主要在配色或侧栏/顶栏：用户需要完成“导入、选择、建组、分流”，而现在界面要求他们先理解对象分类、快捷键和保存层级。两次导航调整没有解决这一点。

## 一、材料索引与观察边界

以下优先使用官方材料。截图用于观察布局，不证明最新安装版的全部行为；开发分支代码证明该分支的组织方式，不代表所有发布版本。没有安装竞品、输入私人订阅或连接自己的核心到公开 Dashboard。

| 材料 | 实际观察到的设计 | 对 sing 的启发 | 不直接照搬 |
|---|---|---|---|
| [sing-box Apple 导航源码](https://raw.githubusercontent.com/SagerNet/sing-box-for-apple/dev/ApplicationLibrary/Views/NavigationPage.swift)、[新建配置表单](https://raw.githubusercontent.com/SagerNet/sing-box-for-apple/dev/ApplicationLibrary/Views/Profile/NewProfileView.swift) | 导航区分 Dashboard、Groups、Connections、Logs、Tools、Settings；配置创建另有 Local/iCloud/Remote 和更新选项 | 日常运行和配置档案分层；尊重原生能力不等于平铺全部原生字段 | 配置文件导入不等同于任意节点订阅导入；不能把它的配置型上手流程完整套给小白 |
| [sing-box Dashboard 导航源码](https://raw.githubusercontent.com/SagerNet/sing-box-dashboard/main/src/App.tsx)、[分组源码](https://raw.githubusercontent.com/SagerNet/sing-box-dashboard/main/src/views/GroupsView.tsx) | Overview/Groups/Connections/Logs/Tools/Settings；组可展开、选择成员、测速，并处理选择失败状态 | 同一组内“当前选择、可用成员、测试”相邻，避免跳转；状态需有加载、空、失败区别 | 公开实例首先要求核心 URL/secret，是控制入口，不是本地首次安装向导 |
| [sing-box Desktop 文档](https://sing-box.sagernet.org/clients/desktop/)、[项目](https://github.com/SagerNet/sing-box-for-desktop) | 文档以本地/远程配置管理为定位；仓库包含 Dashboard 子模块 | 可以共享运行视图和能力模型，不必让每种主机完全重建交互概念 | 文档与 README 对 Linux 支持描述存在差异，本轮不据此承诺其 Linux 发布状态 |
| [Surge 官网界面](https://nssurge.com/)、[策略组手册](https://manual.nssurge.com/policy-groups/overview.html) | 官网 Activity 截图区分接管开关、网络/配置/模式、路由器/DNS/代理延迟与连接/流量；策略组把规则引用和具体出口选择解耦 | 主界面回答“正在发生什么”；问题定位展示路径和证据；规则引用组，日常切换节点不改规则 | 不复制 MITM、脚本重写等与当前目标无关的产品面积；不把 URLTest 冒充 Surge Smart |
| [Clash Verge Rev 入门及官方截图](https://www.clashverge.dev/guide/quickstart.html) | 入门按导入订阅、选择节点/模式、启用接管排列；节点图以代理组和当前成员为主 | 首次使用按任务连成一条路；日常页优先组与成员，不把 direct、节点、组混成配置对象清单 | 外部 Clash 完整配置的订阅语义不能照搬为本产品“节点订阅会接管所有 DNS/路由” |
| [QX 官方配置样例](https://raw.githubusercontent.com/crossutility/Quantumult-X/master/sample.conf) | server_remote、policy、filter_remote 分开；规则资源可指定目标策略；策略可按来源/名称筛选节点 | 订阅是来源，组是出口选择，规则是匹配和动作；规则导入应当能一次完成目标绑定 | 不能把外部格式策略和不支持的条件悄悄简化；本轮未实机操作 QX 的完整配置 UI |
| [Hiddify 官方项目说明](https://github.com/hiddify/hiddify-app) | 项目强调跨格式订阅、自动节点选择、远程档案、更新和订阅用量信息 | 先让用户完成连接；来源健康和更新时间是日常需要的信息，不只显示 JSON 类型 | “自动”不能成为隐藏路由/DNS改动的理由；订阅额度只在来源提供时展示，不能推测 |
| [LazyGit 官方演示](https://raw.githubusercontent.com/jesseduffield/lazygit/assets/demo/commit_and_push-compressed.gif)、[快捷键](https://raw.githubusercontent.com/jesseduffield/lazygit/master/docs/keybindings/Keybindings_en.md) | 所选对象与差异/详情并排；操作作用于焦点；菜单提供 Enter 执行、Esc 取消、搜索 | 列表—详情—操作组成固定语法；应用前展示可读变化；专家快捷键是加速方式 | 不复制全部多面板布局，不要求用户背一屏按键 |
| [K9s commands](https://k9scli.io/topics/commands/) | 上下文帮助、过滤、命令/资源入口、描述与日志、Esc 退出当前层 | 一个可搜索操作入口覆盖低频功能；上下文信息持续可见 | 不把命令模式或危险的免确认操作当作新手默认路径 |
| [btop 官方说明与截图](https://github.com/aristocratos/btop#screenshots) | 紧凑实时图、选中进程详情、排序/筛选；文档明确支持可点击的按键提示 | 状态区稳定、颜色有语义、鼠标是可选增强、列表更新不抢操作焦点 | 不做装饰性的监控墙，不把 CPU/内存图挤进普通代理首页 |
| [Television 官方说明](https://github.com/alexpasmantier/television) | 可扩展数据源的实时模糊搜索，source/preview/action 分离 | 统一节点、成员、目标、命令的搜索选择器 | 不要求用户先写 channel 配置；本轮远端截图未正常渲染，视觉结论不据此产生 |

### 可回看的视觉素材

图片属于原项目，以下仅链接官方公开素材；没有复制成项目资产或用于商标设计。

#### Surge：运行信息的分层

![Surge 官方 Activity 界面](https://nssurge.com/static/media/screenshot.4663424c75b72f9c3901.webp)

看点：接管状态、流量决策模式、诊断、统计有分工。不是让每个底层功能都争夺主入口。图中数值为官方示例，不是本机测量。

#### Clash Verge：组与节点选择

![Clash Verge 官方节点和模式示例](https://www.clashverge.dev/assets/guide/quickstart/verge_proxy.png)

看点：以组为工作单元；当前成员和组类型同时可见。图片中的服务商内容是官方示例，不是推荐。

#### LazyGit：对象和变更相邻

![LazyGit 官方操作演示](https://raw.githubusercontent.com/jesseduffield/lazygit/assets/demo/commit_and_push-compressed.gif)

看点：操作对象、结果预览和执行反馈保持空间关系。只借鉴交互，不复制源码或品牌。

#### btop：高密度但有分区

![btop 官方终端截图](https://raw.githubusercontent.com/aristocratos/btop/main/Img/normal.png)

看点：稳定布局、窄边框、对齐数值、有意义的趋势。我们只需要其中的网络状态设计，不需要整张监控屏。

## 二、当前产品的具体问题

1. **入口按实现分类，不按任务分类。** 11 个同级标签横向摆放后仍然太散。`Outbounds` 同时包括直连对象、代理节点和组，新用户难以找出“换节点”。证据：`src/ui.rs` 的 PAGES 和 rows。
2. **可发现性仍依赖阅读快捷键。** 增加底栏文字只是补救，主体内容里依然缺少可聚焦的 New group / Import / Apply 等操作控件。新用户应能用方向键、Tab、Enter 完成流程。
3. **建组暴露了内部编辑流程。** 先修改 tag、再打开成员窗口、保存成员回表单、再次保存，用户必须理解两个提交层级。
4. **节点订阅和规则资源放在抽象的 Resources 里。** 用户的理解是“添加节点订阅”和“给 YouTube 分流”，不是“创建资源对象”。
5. **应用审阅还不够产品化。** 当前主要列 JSON 路径，而不是“新增 YouTube 规则，使用 Media 组；DNS 未改变；重启会中断连接”。证据：`src/native.rs::review`。
6. **高级功能入口不等于高级工作流完成。** JSON 可编辑保留了表达能力，但还缺字段导航、引用跳转、精确报错、引用安全改名、配置来源解释等体验。
7. **更新与错误缺少就地闭环。** 用户不应为了找到导入失败原因、规则位置或待应用对象在多个页面来回跳。

## 三、产品结构建议

### 原则：一份配置、多个按任务组织的视图

- 普通操作和高级编辑修改同一份原生文档，不另起一套简化配置再反复覆盖。
- “显示名字”和原生 tag 分离；基础表单自动生成稳定标识，专家仍可检查 tag 和引用关系。
- 简单界面表达不了的原生字段保留，并提示存在额外高级配置，而非假装界面展示了全部效果。
- 不设两套互斥的“新手/专家模式”。每个对象按“常用 → 高级 → 原生”逐层展开。

### 六个顶部页面

```text
Home   Proxies   Routing   Subscriptions   Activity   Settings
```

| 页面 | 回答的问题 | 主体内容与主操作 |
|---|---|---|
| Home | 现在能用吗？我接管了哪些流量？ | 核心/接管/检测状态、运行模式、默认出口摘要；Start/Stop、Change proxy、Test connection；未配置时改为简短引导 |
| Proxies | 用哪个节点？怎么建组？ | Groups / All nodes；默认看组及实际选中成员；New group、Test、搜索、节点来源；组内选节点与编辑组结构明确分开 |
| Routing | 哪些流量去哪？ | Rules / Rule sets；规则按真实顺序显示匹配、动作、目标、来源；Add rule / Import rule set / Move；默认目标在列表末尾明确展示 |
| Subscriptions | 我的节点来源是否正常？ | 链接导入、更新时间、节点数、更新变化、冲突/失败状态；不把完整配置档案当成普通节点订阅静默导入 |
| Activity | 哪个连接有问题，证据是什么？ | Connections / Logs / Diagnostics；同一连接的实际规则、组、出口链和进程（若可用）能串起来；日志为深入入口 |
| Settings | 如何接管、解析和运行？ | Network（Capture、Listeners、DNS）、Core、Interface、Advanced/native tools；不把设置项变成全部一级标签 |

DNS 的唯一完整编辑入口为 **Settings → Network → DNS**。Home 可显示 DNS 摘要和跳转，Routing 可关联相应 DNS 规则，但都跳到同一编辑器，不能再复制一份配置页。DNS 编辑器内部仍保留原生 Servers / Rules / Options，不用一个“智能 DNS”开关掩盖真实设置。

规则集的唯一管理入口为 **Routing → Rule sets**；添加分流时内嵌调用同一资源编辑器。跨页面的快捷操作是到同一对象的链接，不是额外状态副本。

这是信息架构提案，不是声称当前 0.5.1 已经有这些页面。

## 四、优先设计三个完整流程

### A. 第一次连接

`Import subscription → Choose initial proxy → Choose capture scope → Review & start`

- 第一屏只要求链接；名称自动建议、可修改。User-Agent 等在 Advanced 内。
- 下载、识别、验证有阶段反馈；失败留住输入，给出重试/详情，不弹一条错误就结束。
- 导入完成立即可“使用这些节点”或“创建组”，不用用户自己找 Outbounds。
- 接管选择用 System proxy / TUN / Proxy ports，并显示对应应用覆盖、权限和 SSH 所控制主机；不把 TUN 当无条件的新手默认。
- 已有配置的用户不能被首次向导重置；引导只为缺失步骤提供可取消的建议。
- 核心启动、系统接管验证、具体目标访问结果分开；不能用一个绿色 Connected 代替三个判断。

### B. 快速建立一个可用组

同一张工作表：`Name → Manual / Automatic → Search and select members → Create group`。

- 成员选择直接嵌入表单；右侧/下方展示已选成员、当前/默认成员，提交只有一个 Create group。
- 支持按名称、来源筛选并多选；默认成员必须来自已选列表。
- 自动组只在 Advanced 展开测试地址、间隔、容差。必须说明是测试延迟策略，不是保证下载最快。
- 以来源过滤“动态跟随订阅”是后续显式能力，不把一次筛选勾选冒充动态组。默认先实现明确的固定成员。
- 保存后显示“已加入草稿”和 Review changes；运行中的组选择可通过 gRPC 快速切换，并单独标记是否成功。

### C. 给 YouTube 分流

在 Routing 选择 **Import rule set**，无需用户先研究 QX、Clash、SRS 分类。

1. 粘贴原始文件链接。
2. 自动识别并显示格式、条数和无法转换的条件；本机转换，不发第三方转换服务。
3. 选择 `Send matching traffic to: Media`，若组不存在，当前流程中直接新建。
4. 预览规则在已有列表中的插入位置，并提示是否可能被前面的无条件规则挡住；不能无脑追加后宣称会生效。
5. 一次确认，原子地保存资源与路由引用；取消任何一步都不留下空组/孤立资源。
6. 清楚展示“DNS 未改变”。若需要补 DNS 规则，以独立、可审阅的建议提供，不能默默绑定两套规则。

原生 sing-box 规则集保留 inline/local/remote 及适用的 source/binary 格式，不经过有损的外部格式转换。复杂条件不能因“易用”而剥离；警告必须能定位受影响条目。

## 五、TUI 的统一交互与视觉语法

- 顶部最多六个主页面，不换行塞 11 个入口；窄屏用短标签或可见的溢出菜单，不移动页面顺序。
- 顶部持久状态只占一行：主机、运行/接管、待应用变化。运行模式与接管方式分开，不重复放两套切换器。
- 主体：页面标题与 1–2 个主操作 → 搜索/筛选 → 列表/详情。宽屏并排，窄屏 Enter 深入，Esc 返回。
- 可见操作控件可通过 Tab 聚焦、Enter 执行；快捷键只是旁注。鼠标可点击作为增强，SSH 无鼠标仍可完成全部主流程。
- 数字切页、Tab 移焦、方向键选项、Space 勾选、Enter 执行可见动作、Esc 返回；所有表单语义一致。
- 可搜索的 Actions 菜单容纳低频操作和专家跳转。既支持输入“import”，也能找到 DNS/规则/原生编辑入口；不强制用户使用命令模式。
- 空状态有一个主操作；加载显示阶段；失败有 Retry / Details；编辑错误定位字段，不把整个表单清空。
- 聚焦行、已选节点、运行值、草稿值用不同标记，不能只靠一个绿色高亮。
- 深色背景、单一强调色、低对比边框；绿色只代表已确认成功，黄色用于待应用/需处理，红色用于失败。颜色旁有文本标记。
- 列表名称左对齐、延迟/流量右对齐；来源在次级信息。不要要求 Nerd Font，不用随机 Emoji 标识协议。
- 更新和测速不改变当前焦点、不自动把正在选择的列表重新排序。
- 底栏只保留当前任务的少数快捷操作，完整帮助属于按需层。专业感来自稳定和信息关系，不来自按钮数量。

### Home 草图（示例数据，未实现）

```text
sing  my-mac                 Running · System proxy verified
Home  Proxies  Routing  Subscriptions  Activity  Settings

Connection                         Routing
Default proxy   Tokyo 01            Rule-based
Capture         System proxy        Unmatched → Main
Last check      Passed · 2 min ago
[Change proxy] [Test connection]    [Stop]

Groups
Main            Tokyo 01            Manual      48 ms
Media           Singapore 02        Automatic   61 ms
Work            Tokyo 01            Manual      48 ms

Changes   1 new rule · not applied             [Review]

Tab Focus    Enter Open    / Search    : Actions    ? Help
```

Home 中的默认出口是路由默认目标，不代表所有请求走这一个节点；组列表可按需展开。真实检查过期、失败、未测试时使用不同文案。组名为示例，不自动替用户创建 Media/Work。

### Routing 草图（示例数据，未实现）

```text
Routing                           [Add rule] [Import rule set]
Rules   Rule sets                                      / Search

Order   Match                     Action / destination
1       Private IPs               Direct
2       YouTube rule set           Media → Singapore 02
3       Work domains               Work → Tokyo 01
        Unmatched traffic          Main → Tokyo 01

Selected: YouTube rule set
Source: Remote QX list · converted locally
Status: Saved draft · not applied
[Change destination] [Move] [View source] [Advanced]
```

sniff、hijack-dns、resolve 等处理动作不能为了简洁从路由顺序中删除或重新排序；实际实现须在完整有序列表保留它们，并用较弱的视觉层级区分“处理动作”和“出口决策”。复杂逻辑保持嵌套显示。

## 六、专家能力应当怎样被看到

1. **引用可追踪**：规则 → 组 → 节点；DNS → detour/bootstrap；节点来自哪个订阅。
2. **状态可证明**：草稿/运行差异、校验输出、实测连接元数据；不可推断时显示 Unknown，而不是自动讲故事。
3. **变化可审阅**：人类可读摘要默认展开，原生 diff 按需查看；明确重启范围和 DNS/接管影响。
4. **高级编辑不丢数据**：未建模字段可见提示、原生编辑和未知字段往返测试；稳定 tag 与显示名分离。
5. **故障可恢复**：失败留输入、更新冲突定位到对象、应用失败反馈恢复结果；不能只写“自动恢复”而不验证。

这些不都已实现。依赖跳转、真正的事务化向导、语义 diff、搜索命令菜单和更完整的检测都属于需要补足的产品能力，不能把原生 JSON 可编辑等同于它们已经存在。

## 七、下一步与验收

建议先制作不连接真实网络的可操作原型，而非直接重排生产 UI：

1. 确认六页结构、DNS 的唯一入口、规则资源归属。
2. 做首次导入、建组、规则绑定三条完整交互，以及 Home/Proxies/Routing 的视觉稿。
3. 用户在不看聊天说明、不查快捷键表的情况下尝试任务；按卡住的位置调整。
4. 通过后接入现有 native 数据与运行管理；不重写已经通过回归的配置底座。

验收目标（待原型测试，不是已经达成的指标）：

- 首次用户能在 30 秒内找到订阅导入入口；完成导入无需理解 inbound/outbound/tag。
- 创建组只有一个最终提交，成员选择可搜索，默认成员有效。
- 导入规则和绑定目标在同一流程完成，并能说清是否已经应用。
- 不用 JSON 完成前三个任务；有额外原生字段的既有配置经过操作后仍保留原意。
- 专家可快速定位原生对象、引用与校验失败字段，而非走冗长强制向导。
- 80×24 完成所有主要流程；更窄终端有明确降级，不遮挡主操作、错误和返回入口。
- 名称含中文/Emoji、200+ 节点、空组、失效订阅、转换损失、API 断开、草稿冲突均有明确状态。
- 键盘是完整主路径；鼠标可用但不是必要条件；只有需要系统授权时才离开 TUI。

本轮仅研究和记录，未修改源代码、未创建生产 UI 原型、未变更任何真实订阅/代理/DNS/TUN，也没有宣称完成新用户可用性测试。
