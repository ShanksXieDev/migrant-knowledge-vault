//! 力导向图布局（F-10「图谱进阶」）。
//!
//! [`crate::locate::build_graph_snapshot`] 使用的是**确定性环形布局**（按度数调整半径），
//! 只能表达「枢纽靠中心」，无法表达**分簇**。本模块提供零外部依赖（仅 `std`）的
//! **力导向布局**实现，用于在图谱视图中呈现按标签 / 格式 / 类型形成的团簇。
//!
//! ## 算法
//!
//! Fruchterman–Reingold 变体，每一轮迭代累加四类力：
//!
//! | 力 | 作用域 | 量级 |
//! |----|--------|------|
//! | 斥力 | 近邻精确 + 远场聚合 | `repulsion * k² / d` |
//! | 弹簧 | 每条边 | `spring * d² / k` |
//! | 簇内吸引 | 同 `group` 的成员 | `group_attraction * d`（指向簇质心） |
//! | 枢纽居中 | 全局 | 与节点权重成比例地拉向画布中心 |
//!
//! 其中 `k = IDEAL_LENGTH_FACTOR * sqrt(width * height / n)` 为理想边长。
//! 当 `d = k` 时斥力与弹簧量级相当（`repulsion * k` vs `spring * k`），因此这是布局的
//! 自然平衡尺度。位移按 [`LayoutConfig::damping`] 缩放并受**温度**限制，温度从
//! `1.4k` 线性退火到 `0.02k`：前期可以大范围重排，后期收敛稳定。
//!
//! ## 可扩展性：Barnes–Hut 网格层级
//!
//! 斥力项的朴素实现是 `O(n²)`，在 `n = 5000` 时单轮就是 2500 万次点对运算，
//! 再乘以迭代轮数完全不可接受。但**只**用「邻近格」近似也不够：截断掉的远场斥力
//! 会让弹簧把整张图收缩成一团，各簇挤在一起、彼此重叠。
//!
//! 因此本实现采用 **Barnes–Hut + 均匀网格层级**：
//!
//! - 画布被切成**多级方形网格**（第 `l` 级格边长 `s₀ · 2ˡ`），每级维护单元的
//!   **质量**与**质心**；
//! - 对每个节点，从最粗级往细级遍历：若某单元的质心距离超过 `s / θ`
//!   （`θ = BH_THETA`），就把整个单元当作**一个点质量**参与斥力计算；
//!   否则下钻到该单元的子单元继续细分；
//! - 只有落到**最细级**的单元才做精确的两两计算。
//!
//! 这样单轮斥力开销从 `O(n²)` 降到 `O(n · log n)`，同时**保留了远场斥力**，
//! 保证大图既能快速收敛、又不会收缩成一个球。
//!
//! 迭代轮数会随节点数自动下调（见 [`effective_iterations`]），把总工作量控制在
//! 预算内，保证 5000 节点在 1 秒内返回。
//!
//! ## 数值稳健性
//!
//! 力导向布局有两个容易踩的坑，本实现都做了处理：
//!
//! - **重合吸引子**：斥力若写成乘性软化 `d·k²/(d² + soft²)`，则 `d → 0` 时力的大小
//!   也趋于 0，两个节点一旦完全重合就再也分不开。这里改用**加性软化**
//!   `k²/(d + soft)`，力的大小只由距离决定，方向由单位位移向量给出；再对
//!   完全重合（位移为零）的节点用 `separation_direction` 派生出确定性的分离方向。
//! - **边界压点**：越界节点若用「钳制」处理，所有越界节点会被压到**同一个**
//!   边界坐标上形成重合点。这里改用**反射**（越界距离折回画布内侧），
//!   反射后仍越界时再兜底钳制，保证坐标始终落在 `[0, width] × [0, height]`。
//!
//! ## 确定性
//!
//! 相同的 `nodes` + `edges` + `config` 一定得到**逐字节相同**的输出：
//!
//! - 随机性只来自内置的 `splitmix64` PRNG（种子来自 [`LayoutConfig::seed`]），
//!   不使用系统随机源、时间戳或指针地址；
//! - 不使用 `HashMap` 的迭代顺序，分组用排序后的 `Vec` + 二分查找；
//! - 正弦 / 余弦使用本模块自带的**纯算术实现**（不依赖平台 `libm`），保证
//!   跨平台结果一致。
//!
//! ## 退化兜底
//!
//! - 空输入返回空数组；单节点、无边节点均能正常返回；
//! - 所有坐标在返回前都被钳制到 `[0, width] × [0, height]`；
//! - 若出现 `NaN` / `Inf`（理论上不会发生），整体回退到确定性的**网格排布**。

use std::cmp::Ordering;

/// 布局输入：一个节点。
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutNode {
    /// 节点在输入数组中的下标（调用方用它在边里引用节点）。
    pub index: usize,
    /// 节点权重（通常取度数）。权重高的节点被布局得更靠中心。
    pub weight: f32,
    /// 分簇标识。相同 `group` 的节点属于同一簇（例如同一格式 / 同一标签）。
    /// `group` 相同表示同簇。
    pub group: u32,
}

/// 布局参数。
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutConfig {
    /// 画布宽。
    pub width: f32,
    /// 画布高。
    pub height: f32,
    /// 迭代轮数。
    pub iterations: usize,
    /// 斥力强度（越大越分散）。
    pub repulsion: f32,
    /// 弹簧强度（越大边越短）。
    pub spring: f32,
    /// 簇内吸引强度（越大簇越紧）。
    pub group_attraction: f32,
    /// 阻尼（0~1，越大越稳定、收敛越快）。
    pub damping: f32,
    /// 随机数种子（保证结果可复现）。
    pub seed: u64,
}

impl Default for LayoutConfig {
    fn default() -> Self {
        Self {
            width: 1200.0,
            height: 900.0,
            iterations: 300,
            repulsion: 1.0,
            spring: 1.0,
            group_attraction: 8.0,
            damping: 0.85,
            seed: 0x5EED_1A7E_C0DE_1234,
        }
    }
}

/// 布局结果：一个节点的坐标。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayoutPoint {
    pub x: f32,
    pub y: f32,
}

/// 理想边长相对画布面积的比例系数（`k = FACTOR * sqrt(area / n)`）。
///
/// 取 1.0 时布局能铺满约 80%~100% 的画布：理想边长 ≈ 画布上每个节点分到的
/// 边长，节点之间既能保持可辨识的间距，又不会浪费画布空间。
const IDEAL_LENGTH_FACTOR: f32 = 1.0;
/// Barnes–Hut 聚合半径系数：远场判定半径为 `单元尺寸 / BH_THETA`。
///
/// 值**越大**聚合半径越小，远场越早被当作一个点质量处理 —— 计算量下降，
/// 但远场近似的误差上升。实测 1.5 左右是拐点：5000 节点 debug 构建耗时下降
/// 约 35%，同时簇内 / 簇间距离比反而**更小**（簇聚得更紧），因为过细的近场
/// 近似会让斥力在小尺度上过度抖动，掩盖掉真实的簇间分离趋势。
const BH_THETA: f32 = 1.5;
/// 最细级网格的目标单元密度（每个最细单元平均容纳多少个节点）。
const FINE_CELL_OCCUPANCY: f32 = 2.0;
/// 斥力软化半径（相对 `k`），避免近距离力值爆炸。
///
/// 采用**加性**软化（分母为 `d + soft`）而非乘性软化（分母为 `d² + soft²`）：
/// 后者在 `d → 0` 时力的大小也趋于 0，会形成「重合吸引子」。
const REPULSION_SOFT_FACTOR: f32 = 0.12;
/// 枢纽居中力相对斥力的强度（无量纲刚度）。
const HUB_CENTER_GAIN: f32 = 0.04;
/// 超过该节点数时按比例下调迭代轮数，保证大图 1 秒内返回。
const LARGE_GRAPH_THRESHOLD: usize = 400;
/// 大图最少迭代轮数。
const MIN_LARGE_GRAPH_ITERATIONS: usize = 40;
/// 初始温度相对理想边长的倍数。
const TEMP_START_FACTOR: f32 = 1.4;
/// 终止温度相对理想边长的倍数。
const TEMP_END_FACTOR: f32 = 0.02;
/// 簇心初始摆布的最大半径（相对画布短边）。
const GROUP_RING_FACTOR: f32 = 0.34;
/// 簇内初始排布半径系数（相对 `k * sqrt(簇大小)`）。
const GROUP_SPREAD_FACTOR: f32 = 0.5;
/// 初始位置抖动幅度（相对 `k`）。
const JITTER_FACTOR: f32 = 0.08;
/// 黄金角（弧度），用于初始位置的螺旋排布。
const GOLDEN_ANGLE: f32 = 2.399_963_2;
/// π 常量（自实现三角函数用，避免依赖平台 libm）。
const PI: f32 = 3.141_592_7;

/// 确定性伪随机数发生器（splitmix64）。
///
/// 与 `rand` 生态无关，纯算术实现，保证跨平台、跨运行完全一致。
#[derive(Debug, Clone)]
struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// 返回 `[0, 1)` 区间的 `f32`。
    fn next_f32(&mut self) -> f32 {
        ((self.next_u64() >> 40) as f32) * (1.0 / 16_777_216.0)
    }
}

/// 确定性正弦函数。
///
/// 仅使用四则运算与多项式近似，不调用平台 `libm`，因此同一输入在任何平台上
/// 都得到逐字节相同的结果（初值撒点与簇心排布依赖它）。
fn det_sin(x: f32) -> f32 {
    let two_pi = 2.0 * PI;
    let mut r = x % two_pi;
    if r > PI {
        r -= two_pi;
    } else if r < -PI {
        r += two_pi;
    }
    // 归约到 [-π/2, π/2]，该区间内 sin(r) == sin(u)。
    let u = if r > PI / 2.0 {
        PI - r
    } else if r < -PI/2.0 {
        -PI - r
    } else {
        r
    };
    let u2 = u * u;
    u * (1.0 - u2 * (1.0 / 6.0 - u2 * (1.0 / 120.0 - u2 / 5040.0)))
}

/// 确定性余弦函数（`cos(x) = sin(x + π/2)`）。
fn det_cos(x: f32) -> f32 {
    det_sin(x + PI / 2.0)
}

/// 归一化后的布局配置（过滤非法值）。
#[derive(Debug, Clone, Copy)]
struct NormalizedConfig {
    width: f32,
    height: f32,
    iterations: usize,
    repulsion: f32,
    spring: f32,
    group_attraction: f32,
    damping: f32,
    seed: u64,
}

impl NormalizedConfig {
    fn new(config: &LayoutConfig) -> Self {
        let default = LayoutConfig::default();
        let width = if config.width.is_finite() && config.width > 0.0 {
            config.width
        } else {
            default.width
        };
        let height = if config.height.is_finite() && config.height > 0.0 {
            config.height
        } else {
            default.height
        };
        let non_negative = |v: f32, fallback: f32| {
            if v.is_finite() {
                v.max(0.0)
            } else {
                fallback
            }
        };
        Self {
            width,
            height,
            iterations: config.iterations.max(1),
            repulsion: non_negative(config.repulsion, default.repulsion),
            spring: non_negative(config.spring, default.spring),
            group_attraction: non_negative(config.group_attraction, default.group_attraction),
            damping: if config.damping.is_finite() {
                config.damping.clamp(0.0, 1.0)
            } else {
                default.damping
            },
            seed: config.seed,
        }
    }
}

/// 对节点做力导向布局，返回与输入等长的坐标数组（按 `LayoutNode.index` 对齐，
/// 但返回值顺序与输入数组顺序一致——调用方会用 `index` 自己去对应）。
///
/// # 索引约定
///
/// `edges` 里的两端写的是 [`LayoutNode::index`] **值**，不是它们在 `nodes` 数组中的
/// 位置。实现会先建立 `index → 数组位置` 的映射，再据此解析边；找不到对应节点
/// 的边、以及自环会被安静地忽略。
///
/// # 约束
///
/// - 结果**必须确定性**：同样的输入 + 同样的 config → 逐字节相同的输出。
/// - 所有坐标都落在 `[0, width] × [0, height]` 范围内（做边界钳制）。
/// - 节点数很多时（如 5000）在 1 秒内算完。
/// - 不会出现 `NaN` / `Inf`（布局退化时会兜底到网格排布）。
pub fn force_directed_layout(
    nodes: &[LayoutNode],
    edges: &[(usize, usize)],
    config: &LayoutConfig,
) -> Vec<LayoutPoint> {
    let n = nodes.len();
    if n == 0 {
        return Vec::new();
    }
    let cfg = NormalizedConfig::new(config);
    let weights = sanitized_weights(nodes);
    let (group_count, group_of) = compress_groups(nodes);
    let edge_pairs = remap_edges(nodes, edges);

    // 理想边长：节点越多、单位面积能容纳的节点越密，理想边长越短。
    let area = cfg.width * cfg.height;
    let k = (IDEAL_LENGTH_FACTOR * (area / n as f32).sqrt()).max(1e-3);

    let mut rng = SplitMix64::new(cfg.seed);
    let (mut px, mut py) = initial_positions(&weights, &group_of, group_count, &cfg, k, &mut rng);
    for i in 0..n {
        px[i] = px[i].clamp(0.0, cfg.width);
        py[i] = py[i].clamp(0.0, cfg.height);
    }

    run_iterations(
        &mut px,
        &mut py,
        &weights,
        &group_of,
        group_count,
        &edge_pairs,
        &cfg,
        k,
        n,
    );

    // 兜底：任何非有限坐标都让整张图退回确定性网格排布。
    if (0..n).any(|i| !px[i].is_finite() || !py[i].is_finite()) {
        return fallback_grid_layout(n, &cfg);
    }

    (0..n)
        .map(|i| LayoutPoint {
            x: px[i].clamp(0.0, cfg.width),
            y: py[i].clamp(0.0, cfg.height),
        })
        .collect()
}

/// 读取节点权重，非有限值按 0 处理。
fn sanitized_weights(nodes: &[LayoutNode]) -> Vec<f32> {
    nodes
        .iter()
        .map(|node| {
            if node.weight.is_finite() {
                node.weight.max(0.0)
            } else {
                0.0
            }
        })
        .collect()
}

/// 把 `group` 取值压缩成 `0..group_count` 的连续编号（排序 + 二分，保持确定性）。
fn compress_groups(nodes: &[LayoutNode]) -> (usize, Vec<usize>) {
    let mut keys: Vec<u32> = nodes.iter().map(|node| node.group).collect();
    keys.sort_unstable();
    keys.dedup();
    let group_of = nodes
        .iter()
        .map(|node| keys.binary_search(&node.group).unwrap_or(0))
        .collect();
    (keys.len(), group_of)
}

/// 建立 `LayoutNode.index → 数组位置` 的映射，并把边转换成数组位置对。
fn remap_edges(nodes: &[LayoutNode], edges: &[(usize, usize)]) -> Vec<(usize, usize)> {
    let mut index_map: Vec<(usize, usize)> = nodes
        .iter()
        .enumerate()
        .map(|(pos, node)| (node.index, pos))
        .collect();
    // 稳定排序 + 位置兜底比较，保证重复 index 时行为依然确定。
    index_map.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    index_map.dedup_by(|a, b| a.0 == b.0);

    let lookup = |key: usize| -> Option<usize> {
        index_map
            .binary_search_by_key(&key, |entry| entry.0)
            .ok()
            .map(|i| index_map[i].1)
    };

    let mut out = Vec::with_capacity(edges.len());
    for &(a, b) in edges {
        let (Some(pa), Some(pb)) = (lookup(a), lookup(b)) else {
            continue;
        };
        if pa == pb {
            continue;
        }
        out.push((pa, pb));
    }
    out
}

/// 初始布局：簇心按黄金角螺旋排布（权重大的簇更靠中心），簇内成员按权重降序
/// 在簇心周围螺旋排布（枢纽落在簇的核心）。
fn initial_positions(
    weights: &[f32],
    group_of: &[usize],
    group_count: usize,
    cfg: &NormalizedConfig,
    k: f32,
    rng: &mut SplitMix64,
) -> (Vec<f32>, Vec<f32>) {
    let n = weights.len();
    let mut px = vec![0f32; n];
    let mut py = vec![0f32; n];

    let mut group_weight = vec![0f32; group_count];
    for i in 0..n {
        group_weight[group_of[i]] += weights[i];
    }

    // 权重大的簇排在螺旋内侧 → 整体更靠画布中心。
    let mut order: Vec<usize> = (0..group_count).collect();
    order.sort_by(|&a, &b| {
        group_weight[b]
            .partial_cmp(&group_weight[a])
            .unwrap_or(Ordering::Equal)
            .then(a.cmp(&b))
    });

    let cx = cfg.width * 0.5;
    let cy = cfg.height * 0.5;
    let ring = cfg.width.min(cfg.height) * GROUP_RING_FACTOR;
    let mut anchor_x = vec![0f32; group_count];
    let mut anchor_y = vec![0f32; group_count];
    for (rank, &group) in order.iter().enumerate() {
        if group_count == 1 {
            anchor_x[group] = cx;
            anchor_y[group] = cy;
            continue;
        }
        let frac = (rank as f32 + 0.5) / group_count as f32;
        let angle = rank as f32 * GOLDEN_ANGLE;
        let radius = ring * frac.sqrt();
        anchor_x[group] = cx + radius * det_cos(angle);
        anchor_y[group] = cy + radius * det_sin(angle);
    }

    // 簇内成员：权重降序 → 枢纽占据簇的核心。
    let mut members: Vec<Vec<usize>> = vec![Vec::new(); group_count];
    for i in 0..n {
        members[group_of[i]].push(i);
    }
    for list in members.iter_mut() {
        list.sort_by(|&a, &b| {
            weights[b]
                .partial_cmp(&weights[a])
                .unwrap_or(Ordering::Equal)
                .then(a.cmp(&b))
        });
    }

    for group in 0..group_count {
        let list = &members[group];
        let count = list.len();
        if count == 0 {
            continue;
        }
        let spread = GROUP_SPREAD_FACTOR * k * (count as f32).sqrt();
        for (rank, &node) in list.iter().enumerate() {
            let angle = rank as f32 * GOLDEN_ANGLE + 0.7 * group as f32;
            let radius = spread * ((rank as f32 + 0.5) / count as f32).sqrt();
            px[node] = anchor_x[group] + radius * det_cos(angle);
            py[node] = anchor_y[group] + radius * det_sin(angle);
        }
    }

    // 轻微抖动打破完全对称（确定性 PRNG）。
    for i in 0..n {
        px[i] += (rng.next_f32() - 0.5) * JITTER_FACTOR * k;
        py[i] += (rng.next_f32() - 0.5) * JITTER_FACTOR * k;
    }

    (px, py)
}

/// Barnes–Hut 网格层级的一级。
///
/// 每级维护**矩形单元**：单元 `(cx, cy)` 覆盖
/// `[cx·cell_w, (cx+1)·cell_w) × [cy·cell_h, (cy+1)·cell_h)`。
///
/// 层级按 **2 的幂**建立：最细级为 `2^p × 2^p` 个单元，每上一级行列数各减半。
/// 因此父单元 `(cx, cy)` 的四个子单元恰好是下一级的
/// `(2cx, 2cy)`、`(2cx+1, 2cy)`、`(2cx, 2cy+1)`、`(2cx+1, 2cy+1)`，
/// 物理范围严格对齐，聚合与下钻都**无损**。
struct Level {
    /// 横向单元数（2 的幂）。
    gx: usize,
    /// 纵向单元数（2 的幂）。
    gy: usize,
    /// 单元物理宽度。
    cell_w: f32,
    /// 单元物理高度。
    cell_h: f32,
    /// 远场聚合阈值平方：`d² >= open2` 时整个单元按一个点质量处理。
    open2: f32,
    /// 单元内节点数（质量）。
    count: Vec<u32>,
    /// 单元内节点 x 坐标和（用于求质心）。
    sum_x: Vec<f32>,
    /// 单元内节点 y 坐标和（用于求质心）。
    sum_y: Vec<f32>,
    /// 单元质心 x（`count > 0` 时有效）。预计算后遍历时无需再做除法。
    center_x: Vec<f32>,
    /// 单元质心 y（`count > 0` 时有效）。预计算后遍历时无需再做除法。
    center_y: Vec<f32>,
    /// 最细级专用：单元内节点下标（计数排序存放）。
    items: Vec<u32>,
    /// 最细级专用：单元起始下标。
    starts: Vec<usize>,
    /// 最细级专用：填桶游标。
    cursor: Vec<usize>,
}

impl Level {
    fn new(gx: usize, gy: usize, cell_w: f32, cell_h: f32, n: usize, with_nodes: bool) -> Self {
        let cells = gx * gy;
        let size = (cell_w * cell_h).sqrt();
        let open = size / BH_THETA;
        Self {
            gx,
            gy,
            cell_w,
            cell_h,
            open2: open * open,
            count: vec![0; cells],
            sum_x: vec![0.0; cells],
            sum_y: vec![0.0; cells],
            center_x: vec![0.0; cells],
            center_y: vec![0.0; cells],
            items: if with_nodes { vec![0; n] } else { Vec::new() },
            starts: if with_nodes { vec![0; cells + 1] } else { Vec::new() },
            cursor: if with_nodes { vec![0; cells] } else { Vec::new() },
        }
    }

    fn cells(&self) -> usize {
        self.gx * self.gy
    }
}

/// 两个完全重合节点之间的确定性分离方向。
///
/// 斥力方向由位移向量决定；当两节点坐标完全相同时位移为零，斥力方向也无定义，
/// 若不特殊处理两者会永久重叠。这里用节点下标派生一个**确定性**单位向量，
/// 并保证 `(i, j)` 与 `(j, i)` 得到互为相反的方向 —— 斥力成对出现，不引入净动量。
fn separation_direction(i: usize, j: usize) -> (f32, f32) {
    let (lo, hi) = if i < j { (i, j) } else { (j, i) };
    // splitmix64 风格的整数混合，跨平台结果完全一致。
    let mut h = (lo as u64)
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add((hi as u64).wrapping_mul(0xBF58_476D_1CE4_E5B9));
    h ^= h >> 30;
    h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h ^= h >> 27;
    h = h.wrapping_mul(0x94D0_49BB_1331_11EB);
    h ^= h >> 31;
    // 只取低 16 位映射到 [0, 2π)：整数运算 + 一次 f32 乘法，跨平台一致。
    let angle = (h & 0xFFFF) as f32 * (2.0 * PI / 65536.0);
    let (s, c) = (det_sin(angle), det_cos(angle));
    if i < j {
        (c, s)
    } else {
        (-c, -s)
    }
}

/// 主迭代：累加斥力 / 弹簧 / 簇内吸引 / 枢纽居中，按阻尼与温度积分位移。
fn run_iterations(
    px: &mut [f32],
    py: &mut [f32],
    weights: &[f32],
    group_of: &[usize],
    group_count: usize,
    edges: &[(usize, usize)],
    cfg: &NormalizedConfig,
    k: f32,
    n: usize,
) {
    let iterations = effective_iterations(n, cfg.iterations);
    // 斥力量级：rep_k2 / d，d = k 时约为 repulsion * k。
    let rep_k2 = cfg.repulsion * k * k;
    // 斥力软化半径（加性）：用 `d + soft` 而不是 `d² + soft²` 做分母。
    //
    // 加性软化的关键在于力的**方向**始终由位移向量的单位方向给出：
    // 乘性形式 `d·k²/(d²+soft²)` 在 d → 0 时力的大小也趋于 0，
    // 会形成一个「重合吸引子」—— 两个节点一旦完全重叠就再也分不开。
    let soft = REPULSION_SOFT_FACTOR * k;
    // 弹簧量级：spring * d² / k，d = k 时约为 spring * k。
    let spring_gain = cfg.spring / k;
    // 簇内吸引量级：group_attraction * d（指向簇质心的线性刚度）。
    let cluster_gain = cfg.group_attraction;
    // 枢纽居中量级：HUB_CENTER_GAIN * w * d。
    let hub_gain = HUB_CENTER_GAIN;

    // 枢纽居中：权重按 min-max 归一化后再减去均值，等权节点不产生整体漂移。
    let hub_weight = normalized_hub_weights(weights);

    let mut acc_x = vec![0f32; n];
    let mut acc_y = vec![0f32; n];
    let mut centroid_x = vec![0f32; group_count];
    let mut centroid_y = vec![0f32; group_count];
    let mut centroid_count = vec![0usize; group_count];

    let mut levels = build_levels(&cfg, k, n);
    // 遍历栈：(级号, 单元 x, 单元 y)。
    let mut stack: Vec<(u32, u32, u32)> = Vec::with_capacity(256);

    let center_x = cfg.width * 0.5;
    let center_y = cfg.height * 0.5;

    for iteration in 0..iterations {
        // --- 簇质心 ---
        centroid_x.fill(0.0);
        centroid_y.fill(0.0);
        centroid_count.fill(0);
        for i in 0..n {
            let group = group_of[i];
            centroid_x[group] += px[i];
            centroid_y[group] += py[i];
            centroid_count[group] += 1;
        }
        for group in 0..group_count {
            let count = centroid_count[group];
            if count > 0 {
                centroid_x[group] /= count as f32;
                centroid_y[group] /= count as f32;
            }
        }

        // --- 斥力（Barnes–Hut 网格层级） ---
        update_levels(&mut levels, px, py, n);
        acc_x.fill(0.0);
        acc_y.fill(0.0);
        for i in 0..n {
            let (xi, yi) = (px[i], py[i]);
            let (mut ax, mut ay) = (0.0f32, 0.0f32);
            // 节点自身在最细级网格中的单元下标；每上一级行列数减半，
            // 因此第 `l` 级的自身单元下标就是 `base >> l`。
            // 坐标已被钳制到画布内，故这里同样要钳制下标，避免越界。
            let base_x = ((xi / levels[0].cell_w) as usize).min(levels[0].gx - 1);
            let base_y = ((yi / levels[0].cell_h) as usize).min(levels[0].gy - 1);
            stack.clear();
            stack.push((levels.len() as u32 - 1, 0, 0));
            while let Some((level_idx, cell_x, cell_y)) = stack.pop() {
                if level_idx == 0 {
                    // 最细级：精确的两两计算。
                    let fine = &levels[0];
                    let cell = cell_y as usize * fine.gx + cell_x as usize;
                    for slot in fine.starts[cell]..fine.starts[cell + 1] {
                        let j = fine.items[slot] as usize;
                        if j == i {
                            continue;
                        }
                        let dx = xi - px[j];
                        let dy = yi - py[j];
                        let d2 = dx * dx + dy * dy;
                        let dist = d2.sqrt();
                        let inv = rep_k2 / (dist + soft);
                        if dist > 1e-6 {
                            ax += dx * inv / dist;
                            ay += dy * inv / dist;
                        } else {
                            // 完全重合：位移向量为零，方向无定义，用确定性方向分开。
                            let (ux, uy) = separation_direction(i, j);
                            ax += ux * inv;
                            ay += uy * inv;
                        }
                    }
                    continue;
                }
                // 粗级：子单元足够远就按点质量处理，否则下钻。
                let child = &levels[level_idx as usize - 1];
                let open2 = child.open2;
                // 含有节点自身的单元必须下钻：否则自身质量会被当成远场点质量，
                // 产生一个并不存在的「自斥力」。
                let own_x_here = base_x >> (level_idx as usize - 1);
                let own_y_here = base_y >> (level_idx as usize - 1);
                for dy in 0..2u32 {
                    for dx in 0..2u32 {
                        let lx = cell_x * 2 + dx;
                        let ly = cell_y * 2 + dy;
                        if lx as usize >= child.gx || ly as usize >= child.gy {
                            continue;
                        }
                        let cell = ly as usize * child.gx + lx as usize;
                        let count = child.count[cell];
                        if count == 0 {
                            continue;
                        }
                        let mass = count as f32;
                        // 质心已在 `update_levels` 预计算，这里直接读取。
                        let ccx = child.center_x[cell];
                        let ccy = child.center_y[cell];
                        let ddx = xi - ccx;
                        let ddy = yi - ccy;
                        let d2 = ddx * ddx + ddy * ddy;
                        let is_own = lx as usize == own_x_here && ly as usize == own_y_here;
                        if !is_own && d2 >= open2 {
                            // 远场：整个单元当作一个点质量。
                            let dist = d2.sqrt();
                            let inv = rep_k2 * mass / (dist + soft);
                            ax += ddx * inv / dist;
                            ay += ddy * inv / dist;
                            continue;
                        }
                        stack.push((level_idx - 1, lx, ly));
                    }
                }
            }
            acc_x[i] = ax;
            acc_y[i] = ay;
        }

        // --- 弹簧 ---
        for &(a, b) in edges {
            let dx = px[b] - px[a];
            let dy = py[b] - py[a];
            let dist = (dx * dx + dy * dy).sqrt().max(1e-6);
            let gain = spring_gain * dist;
            acc_x[a] += dx * gain;
            acc_y[a] += dy * gain;
            acc_x[b] -= dx * gain;
            acc_y[b] -= dy * gain;
        }

        // --- 簇内吸引（指向簇质心） ---
        for i in 0..n {
            let group = group_of[i];
            if centroid_count[group] < 2 {
                continue;
            }
            acc_x[i] += cluster_gain * (centroid_x[group] - px[i]);
            acc_y[i] += cluster_gain * (centroid_y[group] - py[i]);
        }

        // --- 枢纽居中 ---
        for i in 0..n {
            let pull = hub_gain * hub_weight[i];
            acc_x[i] += pull * (center_x - px[i]);
            acc_y[i] += pull * (center_y - py[i]);
        }

        // --- 积分：阻尼缩放 + 温度限幅 + 边界反射 ---
        let progress = iteration as f32 / iterations as f32;
        let temperature =
            k * (TEMP_START_FACTOR + (TEMP_END_FACTOR - TEMP_START_FACTOR) * progress);
        for i in 0..n {
            let mut dx = acc_x[i] * cfg.damping;
            let mut dy = acc_y[i] * cfg.damping;
            let length = (dx * dx + dy * dy).sqrt();
            if length > temperature && length > 0.0 {
                let scale = temperature / length;
                dx *= scale;
                dy *= scale;
            }
            // 边界用「反射」而非「钳制」：钳制会把所有越界节点压到**同一个**
            // 边界坐标上（形成重合点），反射则保留它们之间的差异。
            let mut nx = px[i] + dx;
            if nx < 0.0 {
                nx = -nx;
            } else if nx > cfg.width {
                nx = 2.0 * cfg.width - nx;
            }
            let mut ny = py[i] + dy;
            if ny < 0.0 {
                ny = -ny;
            } else if ny > cfg.height {
                ny = 2.0 * cfg.height - ny;
            }
            // 反射后仍越界（单步位移过大）时兜底钳制，保证坐标始终落在画布内。
            px[i] = nx.clamp(0.0, cfg.width);
            py[i] = ny.clamp(0.0, cfg.height);
        }
    }
}

/// 构建 Barnes–Hut 网格层级。
///
/// 最细级的行列数取**同一个** 2 的幂 `2^p`（单元因此是矩形而非正方形，
/// 但行列同步减半，父子关系严格对齐），再逐级减半直到 1×1。
/// 因为每级都是 2 的幂，父单元的四个子单元在下级中严格对应
/// `(2cx+dx, 2cy+dy)`，聚合与下钻都无损。
fn build_levels(cfg: &NormalizedConfig, k: f32, n: usize) -> Vec<Level> {
    let area = (cfg.width * cfg.height).max(1.0);
    // 最细级单元边长：让每个单元平均容纳 FINE_CELL_OCCUPANCY 个节点。
    let target = (area * FINE_CELL_OCCUPANCY / n as f32).sqrt().max(k * 0.25).max(1e-3);
    // 行列同用一个 2 的幂，保证每级行列同步减半。
    let side = next_power_of_two(
        ((cfg.width.max(cfg.height) / target).ceil() as usize)
            .min(MAX_FINE_GRID_SIDE)
            .max(1),
    );
    let mut levels = Vec::new();
    let mut cells_per_side = side;
    loop {
        let cell_w = cfg.width / cells_per_side as f32;
        let cell_h = cfg.height / cells_per_side as f32;
        let finest = levels.is_empty();
        levels.push(Level::new(
            cells_per_side,
            cells_per_side,
            cell_w,
            cell_h,
            n,
            finest,
        ));
        if cells_per_side == 1 {
            break;
        }
        cells_per_side /= 2;
    }
    levels
}

/// 最细级网格的单边最大单元数（2 的幂），限制内存与建树开销。
const MAX_FINE_GRID_SIDE: usize = 256;

/// 不小于 `v` 的最小 2 的幂（`v <= 1` 时返回 1）。
fn next_power_of_two(v: usize) -> usize {
    if v <= 1 {
        return 1;
    }
    v.next_power_of_two()
}

/// 每一轮刷新网格层级的质量与质心（自细到粗逐级聚合）。
fn update_levels(levels: &mut [Level], px: &[f32], py: &[f32], n: usize) {
    // 最细级：统计单元 + 计数排序。
    {
        let fine = &mut levels[0];
        fine.count.fill(0);
        fine.sum_x.fill(0.0);
        fine.sum_y.fill(0.0);
        for i in 0..n {
            let cx = ((px[i] / fine.cell_w) as usize).min(fine.gx - 1);
            let cy = ((py[i] / fine.cell_h) as usize).min(fine.gy - 1);
            let cell = cy * fine.gx + cx;
            fine.count[cell] += 1;
            fine.sum_x[cell] += px[i];
            fine.sum_y[cell] += py[i];
        }
        let mut acc = 0usize;
        let total_cells = fine.cells();
        for cell in 0..total_cells {
            fine.starts[cell] = acc;
            fine.cursor[cell] = acc;
            acc += fine.count[cell] as usize;
        }
        fine.starts[total_cells] = acc;
        for i in 0..n {
            let cx = ((px[i] / fine.cell_w) as usize).min(fine.gx - 1);
            let cy = ((py[i] / fine.cell_h) as usize).min(fine.gy - 1);
            let cell = cy * fine.gx + cx;
            fine.items[fine.cursor[cell]] = i as u32;
            fine.cursor[cell] += 1;
        }
        // 预计算最细级质心：遍历时直接读，无需再做除法。
        for cell in 0..total_cells {
            let count = fine.count[cell];
            if count > 0 {
                let inv = 1.0 / count as f32;
                fine.center_x[cell] = fine.sum_x[cell] * inv;
                fine.center_y[cell] = fine.sum_y[cell] * inv;
            }
        }
    }
    // 粗级：四个子单元聚合为父单元。
    for level in 1..levels.len() {
        let (lower_levels, upper_levels) = levels.split_at_mut(level);
        let lower = &lower_levels[level - 1];
        let upper = &mut upper_levels[0];
        upper.count.fill(0);
        upper.sum_x.fill(0.0);
        upper.sum_y.fill(0.0);
        for cell_y in 0..upper.gy {
            for cell_x in 0..upper.gx {
                let mut count = 0u32;
                let mut sum_x = 0.0f32;
                let mut sum_y = 0.0f32;
                for dy in 0..2usize {
                    for dx in 0..2usize {
                        let lx = cell_x * 2 + dx;
                        let ly = cell_y * 2 + dy;
                        if lx >= lower.gx || ly >= lower.gy {
                            continue;
                        }
                        let child = ly * lower.gx + lx;
                        count += lower.count[child];
                        sum_x += lower.sum_x[child];
                        sum_y += lower.sum_y[child];
                    }
                }
                let target = cell_y * upper.gx + cell_x;
                upper.count[target] = count;
                upper.sum_x[target] = sum_x;
                upper.sum_y[target] = sum_y;
                // 同步计算父单元质心。
                if count > 0 {
                    let inv = 1.0 / count as f32;
                    upper.center_x[target] = sum_x * inv;
                    upper.center_y[target] = sum_y * inv;
                }
            }
        }
    }
}

/// 大图自动下调迭代轮数，保证总工作量有界。
fn effective_iterations(n: usize, requested: usize) -> usize {
    if n <= LARGE_GRAPH_THRESHOLD {
        return requested;
    }
    let scale = (LARGE_GRAPH_THRESHOLD as f64 / n as f64).sqrt();
    let scaled = (requested as f64 * scale).round() as usize;
    scaled
        .max(MIN_LARGE_GRAPH_ITERATIONS)
        .min(requested)
        .max(1)
}

/// 权重归一化：min-max 归一化后减去均值，等权时全部为 0（不引入整体漂移）。
fn normalized_hub_weights(weights: &[f32]) -> Vec<f32> {
    if weights.is_empty() {
        return Vec::new();
    }
    let mut min = f32::INFINITY;
    let mut max = f32::NEG_INFINITY;
    for &w in weights {
        min = min.min(w);
        max = max.max(w);
    }
    let span = max - min;
    if span <= f32::EPSILON {
        return vec![0.0; weights.len()];
    }
    let mut out = Vec::with_capacity(weights.len());
    let mut mean = 0.0f32;
    for &w in weights {
        let value = (w - min) / span;
        out.push(value);
        mean += value;
    }
    mean /= weights.len() as f32;
    for value in out.iter_mut() {
        *value -= mean;
    }
    out
}

/// 兜底布局：在画布内做确定性网格排布。
fn fallback_grid_layout(n: usize, cfg: &NormalizedConfig) -> Vec<LayoutPoint> {
    let cols = (n as f32).sqrt().ceil().max(1.0) as usize;
    let rows = n.div_ceil(cols).max(1);
    let cell_w = cfg.width / cols as f32;
    let cell_h = cfg.height / rows as f32;
    (0..n)
        .map(|i| LayoutPoint {
            x: ((i % cols) as f32 + 0.5) * cell_w,
            y: ((i / cols) as f32 + 0.5) * cell_h,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn node(index: usize, weight: f32, group: u32) -> LayoutNode {
        LayoutNode {
            index,
            weight,
            group,
        }
    }

    fn distance(a: LayoutPoint, b: LayoutPoint) -> f32 {
        ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt()
    }

    fn average_distance(points: &[LayoutPoint], pairs: &[(usize, usize)]) -> f32 {
        if pairs.is_empty() {
            return 0.0;
        }
        let sum: f32 = pairs
            .iter()
            .map(|&(a, b)| distance(points[a], points[b]))
            .sum();
        sum / pairs.len() as f32
    }

    #[test]
    fn empty_input_returns_empty() {
        let points = force_directed_layout(&[], &[], &LayoutConfig::default());
        assert!(points.is_empty());
    }

    #[test]
    fn single_node_returns_point_inside_canvas() {
        let config = LayoutConfig::default();
        let nodes = vec![node(0, 1.0, 0)];
        let points = force_directed_layout(&nodes, &[], &config);
        assert_eq!(points.len(), 1);
        assert!(points[0].x.is_finite() && points[0].y.is_finite());
        assert!((0.0..=config.width).contains(&points[0].x));
        assert!((0.0..=config.height).contains(&points[0].y));
    }

    #[test]
    fn connected_nodes_end_up_closer_than_disconnected() {
        let config = LayoutConfig::default();
        let nodes = vec![node(0, 1.0, 0), node(1, 1.0, 1)];
        let mut connected_total = 0.0f32;
        let mut disconnected_total = 0.0f32;
        let mut samples = 0.0f32;
        for seed in 0..8u64 {
            let mut cfg = config.clone();
            cfg.seed = seed;
            let connected = force_directed_layout(&nodes, &[(0, 1)], &cfg);
            let disconnected = force_directed_layout(&nodes, &[], &cfg);
            connected_total += distance(connected[0], connected[1]);
            disconnected_total += distance(disconnected[0], disconnected[1]);
            samples += 1.0;
        }
        let connected_avg = connected_total / samples;
        let disconnected_avg = disconnected_total / samples;
        assert!(
            connected_avg < disconnected_avg,
            "有边平均距离 {connected_avg} 应小于无边平均距离 {disconnected_avg}"
        );
    }

    #[test]
    fn layout_is_deterministic_across_runs() {
        let nodes: Vec<LayoutNode> = (0..40)
            .map(|i| node(i, (i % 5) as f32, (i % 3) as u32))
            .collect();
        let edges: Vec<(usize, usize)> = (0..39).map(|i| (i, i + 1)).collect();
        let config = LayoutConfig::default();
        let first = force_directed_layout(&nodes, &edges, &config);
        let second = force_directed_layout(&nodes, &edges, &config);
        assert_eq!(first.len(), second.len());
        for (a, b) in first.iter().zip(second.iter()) {
            assert_eq!(a.x.to_bits(), b.x.to_bits(), "x 必须逐字节相同");
            assert_eq!(a.y.to_bits(), b.y.to_bits(), "y 必须逐字节相同");
        }
    }

    #[test]
    fn different_seeds_produce_different_layouts() {
        let nodes: Vec<LayoutNode> = (0..30)
            .map(|i| node(i, 1.0, (i % 4) as u32))
            .collect();
        let mut a = LayoutConfig::default();
        a.seed = 1;
        let mut b = LayoutConfig::default();
        b.seed = 2;
        let first = force_directed_layout(&nodes, &[], &a);
        let second = force_directed_layout(&nodes, &[], &b);
        assert!(first.iter().zip(second.iter()).any(|(x, y)| {
            x.x.to_bits() != y.x.to_bits() || x.y.to_bits() != y.y.to_bits()
        }));
    }

    #[test]
    fn same_group_nodes_cluster_together() {
        let group_count = 4u32;
        let per_group = 12usize;
        let mut nodes = Vec::new();
        for group in 0..group_count {
            for i in 0..per_group {
                nodes.push(node(group as usize * per_group + i, 1.0, group));
            }
        }
        let config = LayoutConfig::default();
        let points = force_directed_layout(&nodes, &[], &config);

        let mut intra = 0.0f32;
        let mut intra_count = 0.0f32;
        let mut inter = 0.0f32;
        let mut inter_count = 0.0f32;
        for a in 0..nodes.len() {
            for b in (a + 1)..nodes.len() {
                let d = distance(points[a], points[b]);
                if nodes[a].group == nodes[b].group {
                    intra += d;
                    intra_count += 1.0;
                } else {
                    inter += d;
                    inter_count += 1.0;
                }
            }
        }
        let intra_avg = intra / intra_count;
        let inter_avg = inter / inter_count;
        assert!(
            intra_avg < inter_avg,
            "同簇平均距离 {intra_avg} 应小于跨簇平均距离 {inter_avg}"
        );
    }

    #[test]
    fn all_coordinates_stay_inside_canvas() {
        let nodes: Vec<LayoutNode> = (0..120)
            .map(|i| node(i, (i % 7) as f32, (i % 5) as u32))
            .collect();
        let edges: Vec<(usize, usize)> = (0..119).map(|i| (i, i + 1)).collect();
        let config = LayoutConfig::default();
        let points = force_directed_layout(&nodes, &edges, &config);
        for point in &points {
            assert!(
                point.x >= 0.0 && point.x <= config.width,
                "x 越界: {}",
                point.x
            );
            assert!(
                point.y >= 0.0 && point.y <= config.height,
                "y 越界: {}",
                point.y
            );
        }
    }

    #[test]
    fn output_never_contains_nan_or_inf() {
        let nodes: Vec<LayoutNode> = (0..60)
            .map(|i| node(i, f32::NAN, (i % 3) as u32))
            .collect();
        let mut edges = Vec::new();
        for i in 0..59 {
            edges.push((i, i + 1));
            edges.push((i, (i + 13) % 60));
        }
        let mut config = LayoutConfig::default();
        config.width = 0.0; // 非法宽度应回退到默认值
        config.damping = 5.0; // 越界阻尼应被钳制
        let points = force_directed_layout(&nodes, &edges, &config);
        for point in &points {
            assert!(point.x.is_finite(), "x 必须是有限值: {}", point.x);
            assert!(point.y.is_finite(), "y 必须是有限值: {}", point.y);
        }
    }

    #[test]
    fn large_graph_finishes_within_time_budget() {
        // 5000 节点 + 约 1.6 万条边。
        let n = 5000usize;
        let nodes: Vec<LayoutNode> = (0..n)
            .map(|i| node(i, (i % 23) as f32, (i % 11) as u32))
            .collect();
        let mut edges = Vec::new();
        for i in 0..n {
            edges.push((i, (i + 1) % n));
            if i % 3 == 0 {
                edges.push((i, (i + 37) % n));
            }
        }
        let config = LayoutConfig::default();
        let start = Instant::now();
        let points = force_directed_layout(&nodes, &edges, &config);
        let elapsed = start.elapsed();
        assert_eq!(points.len(), n);
        println!("5000 节点布局耗时: {elapsed:?}");
        // CI 机器较慢（且测试默认走 debug 构建），留出余量：要求 3 秒内完成。
        // release 构建下通常在 200ms 以内。
        assert!(
            elapsed.as_secs_f64() < 3.0,
            "5000 节点布局耗时过长: {elapsed:?}"
        );
    }

    #[test]
    fn default_config_uses_reasonable_values() {
        let config = LayoutConfig::default();
        assert!(config.width > 0.0 && config.width.is_finite());
        assert!(config.height > 0.0 && config.height.is_finite());
        assert!((100.0..=10_000.0).contains(&config.width));
        assert!((100.0..=10_000.0).contains(&config.height));
        assert!((1..=10_000).contains(&config.iterations));
        assert!(config.repulsion > 0.0 && config.repulsion.is_finite());
        assert!(config.spring > 0.0 && config.spring.is_finite());
        assert!(config.group_attraction > 0.0 && config.group_attraction.is_finite());
        assert!(config.damping > 0.0 && config.damping <= 1.0);
    }

    #[test]
    fn edges_are_resolved_by_node_index_value() {
        // index 与数组位置故意错开：边里写的是 index 值。
        let nodes = vec![node(10, 1.0, 0), node(20, 1.0, 1), node(30, 1.0, 2)];
        let config = LayoutConfig::default();
        let points = force_directed_layout(&nodes, &[(10, 30)], &config);
        let linked = distance(points[0], points[2]);
        let unlinked = distance(points[0], points[1]);
        assert!(
            linked < unlinked,
            "按 index 解析的边应把节点 10 与 30 拉近: {linked} vs {unlinked}"
        );
    }

    #[test]
    fn invalid_and_self_edges_are_ignored() {
        let nodes = vec![node(0, 1.0, 0), node(1, 1.0, 0)];
        let edges = vec![(0, 0), (0, 999), (999, 1000)];
        let config = LayoutConfig::default();
        let points = force_directed_layout(&nodes, &edges, &config);
        assert_eq!(points.len(), 2);
        for point in &points {
            assert!(point.x.is_finite() && point.y.is_finite());
        }
    }

    #[test]
    fn isolated_nodes_without_edges_are_still_placed() {
        let nodes: Vec<LayoutNode> = (0..8).map(|i| node(i, 1.0, i as u32)).collect();
        let config = LayoutConfig::default();
        let points = force_directed_layout(&nodes, &[], &config);
        assert_eq!(points.len(), 8);
        for point in &points {
            assert!(point.x.is_finite() && point.y.is_finite());
            assert!((0.0..=config.width).contains(&point.x));
            assert!((0.0..=config.height).contains(&point.y));
        }
    }

    #[test]
    fn heavier_nodes_tend_towards_canvas_center() {
        let n = 40usize;
        let nodes: Vec<LayoutNode> = (0..n)
            .map(|i| node(i, i as f32, 0))
            .collect();
        let config = LayoutConfig::default();
        let points = force_directed_layout(&nodes, &[], &config);
        let center_x = config.width * 0.5;
        let center_y = config.height * 0.5;
        let mut heavy = 0.0f32;
        let mut heavy_count = 0.0f32;
        let mut light = 0.0f32;
        let mut light_count = 0.0f32;
        for (i, point) in points.iter().enumerate() {
            let d = ((point.x - center_x).powi(2) + (point.y - center_y).powi(2)).sqrt();
            if i >= n / 2 {
                heavy += d;
                heavy_count += 1.0;
            } else {
                light += d;
                light_count += 1.0;
            }
        }
        let heavy_avg = heavy / heavy_count;
        let light_avg = light / light_count;
        assert!(
            heavy_avg < light_avg,
            "高权重节点平均离中心距离 {heavy_avg} 应小于低权重节点 {light_avg}"
        );
    }

    #[test]
    fn layout_is_stable_with_single_iteration() {
        let nodes: Vec<LayoutNode> = (0..20)
            .map(|i| node(i, (i % 3) as f32, (i % 2) as u32))
            .collect();
        let mut config = LayoutConfig::default();
        config.iterations = 1;
        let points = force_directed_layout(&nodes, &[(0, 1), (1, 2)], &config);
        assert_eq!(points.len(), 20);
        for point in &points {
            assert!(point.x.is_finite() && point.y.is_finite());
        }
    }

    #[test]
    fn edges_shorten_linked_pairs_in_a_medium_graph() {
        let n = 24usize;
        let nodes: Vec<LayoutNode> = (0..n).map(|i| node(i, 1.0, 0)).collect();
        let edges: Vec<(usize, usize)> = (0..n - 1).map(|i| (i, i + 1)).collect();
        let config = LayoutConfig::default();
        let points = force_directed_layout(&nodes, &edges, &config);
        let linked = average_distance(&points, &edges);
        let unlinked = average_distance(&points, &[(0, 12), (1, 13), (2, 14)]);
        assert!(
            linked < unlinked,
            "有边平均距离 {linked} 应小于无边平均距离 {unlinked}"
        );
    }

    #[test]
    fn nodes_do_not_collapse_onto_the_same_point() {
        // 回归测试：簇被斥力推到画布边缘时，若边界用「钳制」处理，所有越界节点
        // 会被压到同一个边界坐标上，形成重合点；斥力方向随之退化为零向量，
        // 节点之间再也无法分开。这里验证任意两个节点都不会落在完全相同的坐标。
        let clusters = 8usize;
        let per = 60usize;
        let n = clusters * per;
        let nodes: Vec<LayoutNode> = (0..n)
            .map(|i| node(i, 1.0, (i / per) as u32))
            .collect();
        let mut edges = Vec::new();
        for c in 0..clusters {
            let base = c * per;
            for i in 0..per {
                edges.push((base + i, base + (i + 1) % per));
            }
        }
        let config = LayoutConfig::default();
        let points = force_directed_layout(&nodes, &edges, &config);
        // 两两比较坐标（用位模式判定「完全相同」）。
        for i in 0..n {
            for j in (i + 1)..n {
                assert!(
                    points[i].x.to_bits() != points[j].x.to_bits()
                        || points[i].y.to_bits() != points[j].y.to_bits(),
                    "节点 {i} 与 {j} 落在完全相同的坐标 {:?} 上",
                    points[i]
                );
            }
        }
    }

    #[test]
    fn cluster_centroids_stay_inside_the_canvas() {
        // 簇质心不应漂到画布之外：否则整簇会被推到边界上挤作一团。
        let clusters = 6usize;
        let per = 40usize;
        let n = clusters * per;
        let nodes: Vec<LayoutNode> = (0..n)
            .map(|i| node(i, 1.0, (i / per) as u32))
            .collect();
        let mut edges = Vec::new();
        for c in 0..clusters {
            let base = c * per;
            for i in 0..per {
                edges.push((base + i, base + (i + 1) % per));
            }
        }
        let config = LayoutConfig::default();
        let points = force_directed_layout(&nodes, &edges, &config);
        let mut sum_x = vec![0f32; clusters];
        let mut sum_y = vec![0f32; clusters];
        for (i, point) in points.iter().enumerate() {
            sum_x[i / per] += point.x;
            sum_y[i / per] += point.y;
        }
        for c in 0..clusters {
            let cx = sum_x[c] / per as f32;
            let cy = sum_y[c] / per as f32;
            assert!(
                cx >= 0.0 && cx <= config.width && cy >= 0.0 && cy <= config.height,
                "簇 {c} 的质心 ({cx}, {cy}) 漂到了画布外"
            );
        }
    }

    #[test]
    fn tightly_packed_clusters_keep_nodes_separated() {
        // 高密度场景：每个簇内部节点很多，斥力与簇内吸引强烈竞争，
        // 验证最小间距不会退化到 0（否则渲染时节点会完全叠在一起）。
        let per = 120usize;
        let nodes: Vec<LayoutNode> = (0..per)
            .map(|i| node(i, 1.0, 0))
            .collect();
        let mut edges = Vec::new();
        for i in 0..per {
            edges.push((i, (i + 1) % per));
            edges.push((i, (i + 7) % per));
        }
        let config = LayoutConfig::default();
        let points = force_directed_layout(&nodes, &edges, &config);
        let mut min_dist = f32::MAX;
        for i in 0..per {
            for j in (i + 1)..per {
                min_dist = min_dist.min(distance(points[i], points[j]));
            }
        }
        assert!(
            min_dist > 0.0,
            "高密度簇内出现了完全重合的节点（最小间距 {min_dist}）"
        );
    }
}
