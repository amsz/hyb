<script setup>
import { computed, onMounted, ref, watch } from "vue";
import { onBeforeRouteLeave } from "vue-router";
import { getStatsHistory } from "../lib/ipc";
import { useAppStore } from "../store/app";

const store = useAppStore();
const history = ref([]);

onMounted(async () => {
  store.refreshStats();
  history.value = await getStatsHistory();
});

/** "3 小时 5 分" / "45 分钟" */
function formatDuration(totalSeconds) {
  const m = Math.floor(totalSeconds / 3600);
  const b = Math.floor((totalSeconds % 3600) / 60);
  return m > 0 ? `${m}小时${b}分` : `${b} 分钟`;
}

/** 紧凑时长 "5h10m" / "45m" / "5h"，只用于卡片标签行右侧的“在座”小字 */
function formatCompact(totalSeconds) {
  const h = Math.floor((totalSeconds || 0) / 3600);
  const m = Math.floor(((totalSeconds || 0) % 3600) / 60);
  if (h > 0) return m > 0 ? `${h}h${m}m` : `${h}h`;
  return `${m}m`;
}

const iso = (d) =>
  `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;

/** 今日统计卡片 */
const cards = computed(() => {
  const s = store.stats;
  if (!s) return [];
  const list = [
    {
      label: "有效工时",
      value: formatDuration(s.workSeconds),
      extra: `在座 ${formatCompact(s.sessionSeconds ?? 0)}`,
    },
    { label: "已完成休息", value: `${s.restCompleted} 次` },
    { label: "跳过休息", value: `${s.restSkipped} 次` },
  ];
  if (store.config?.inputStatsEnabled) {
    list.push({ label: "击键数", value: s.keystrokes.toLocaleString() });
    list.push({ label: "鼠标点击", value: s.mouseClicks.toLocaleString() });
  }
  list.push({ label: "护眼时长", value: formatDuration(s.eyeFilterSeconds) });
  return list;
});

const byDate = computed(() => {
  const map = new Map();
  for (const row of history.value) map.set(row.date, row);
  return map;
});

const WEEK = ["日", "一", "二", "三", "四", "五", "六"];

/** 最近 7 天柱状图数据（柱高 = 当天工作时长，数字 = 完成休息次数） */
const chart = computed(() => {
  const rows = [];
  const today = new Date();
  const days = [];
  for (let i = 6; i >= 0; i--) {
    const d = new Date(today);
    d.setDate(today.getDate() - i);
    days.push({ date: iso(d), label: WEEK[d.getDay()], stat: byDate.value.get(iso(d)), d });
  }
  const max = Math.max(1, ...days.map((x) => x.stat?.workSeconds ?? 0));
  for (const x of days) {
    const work = x.stat?.workSeconds ?? 0;
    const done = x.stat?.restCompleted ?? 0;
    rows.push({
      date: x.date,
      label: x.label,
      isToday: x.date === iso(today),
      height: work === 0 ? "2px" : `${Math.max(4, (work / max) * 100)}%`,
      completed: done,
      tooltip: `${x.date} · ${formatDuration(work)} · 完成 ${done} 次`,
    });
  }
  return rows;
});

const hasData = computed(() =>
  chart.value.some((x) => x.completed > 0 || x.height !== "2px"),
);

/** 连续使用天数（当日有工作时长才算） */
const streak = computed(() => {
  const map = byDate.value;
  const d = new Date();
  if (!map.get(iso(d))?.workSeconds) d.setDate(d.getDate() - 1);
  let n = 0;
  while (map.get(iso(d))?.workSeconds) {
    n += 1;
    d.setDate(d.getDate() - 1);
  }
  return n;
});

// ---- 休息计划（从宠物页迁入） ----
const plan = ref(null);
const rangeError = ref("");
const saving = ref(false);
const saveError = ref("");
/** 最近一次保存成功的时间，用于给“已保存”一个可见凭据 */
const savedAt = ref(null);
const pad2 = (n) => String(n).padStart(2, "0");
const hhmm = (d) => `${pad2(d.getHours())}:${pad2(d.getMinutes())}`;

/** 星期日序：1=周一 … 7=周日（与后端 workDays 一致） */
const DAYS = [
  { n: 1, label: "一" },
  { n: 2, label: "二" },
  { n: 3, label: "三" },
  { n: 4, label: "四" },
  { n: 5, label: "五" },
  { n: 6, label: "六" },
  { n: 7, label: "日" },
];

function normalizeRanges(list) {
  const valid = (Array.isArray(list) ? list : [])
    .filter((r) => r?.start && r?.end && r.start < r.end)
    .map((r) => ({ ...r }));
  return valid.length ? valid : [{ start: "09:00", end: "18:00" }];
}

function planFromConfig(c) {
  return {
    workMinutes: c.workMinutes,
    shortRestMinutes: c.shortRestMinutes,
    longRestEvery: c.longRestEvery,
    longRestMinutes: c.longRestMinutes,
    activeIdleSecs: c.activeIdleSecs ?? 30,
    idlePauseMinutes: c.idlePauseMinutes,
    workScheduleEnabled: c.workScheduleEnabled ?? false,
    workRanges: normalizeRanges(c.workRanges),
    workDays: [...(c.workDays?.length ? c.workDays : DAYS.map((d) => d.n))].sort((a, b) => a - b),
  };
}

/** 有未保存的改动（用于按钮置灰/提示/离开确认） */
const dirty = computed(() => {
  const p = plan.value;
  const c = store.config;
  if (!p || !c) return false;
  const saved = planFromConfig(c);
  return (
    p.workMinutes !== saved.workMinutes ||
    p.shortRestMinutes !== saved.shortRestMinutes ||
    p.longRestEvery !== saved.longRestEvery ||
    p.longRestMinutes !== saved.longRestMinutes ||
    p.activeIdleSecs !== saved.activeIdleSecs ||
    p.idlePauseMinutes !== saved.idlePauseMinutes ||
    !!p.workScheduleEnabled !== !!saved.workScheduleEnabled ||
    JSON.stringify(p.workRanges) !== JSON.stringify(saved.workRanges) ||
    JSON.stringify(p.workDays) !== JSON.stringify(saved.workDays)
  );
});

watch(
  () => store.config,
  (c) => {
    if (!c) return;
    // 别处保存配置（如切暂停/改宠物）会广播 config:changed；
    // 本地有未保存改动时保留用户正在编辑的内容
    if (dirty.value && plan.value) return;
    plan.value = planFromConfig(c);
  },
  { immediate: true },
);

// 有未保存改动时离开健康页先问一句，避免“改完就没了”
onBeforeRouteLeave(() => {
  if (!dirty.value || saving.value) return true;
  return window.confirm("休息计划有未保存的修改，确定离开？");
});

function setRangeStart(i, e) {
  if (!plan.value?.workScheduleEnabled) return; // 未启用时只显示不响应
  const v = e.target.value;
  if (!v) return;
  const r = plan.value.workRanges[i];
  if (r.end && v >= r.end) {
    rangeError.value = "结束时间需晚于开始时间，暂不支持跨午夜";
    return;
  }
  rangeError.value = "";
  plan.value.workRanges = plan.value.workRanges.map((x, j) => (j === i ? { ...x, start: v } : x));
}
function setRangeEnd(i, e) {
  if (!plan.value?.workScheduleEnabled) return;
  const v = e.target.value;
  if (!v) return;
  const r = plan.value.workRanges[i];
  if (r.start && v <= r.start) {
    rangeError.value = "结束时间需晚于开始时间，暂不支持跨午夜";
    return;
  }
  rangeError.value = "";
  plan.value.workRanges = plan.value.workRanges.map((x, j) => (j === i ? { ...x, end: v } : x));
}
function toggleDay(n) {
  if (!plan.value?.workScheduleEnabled) return;
  const days = plan.value.workDays;
  if (days.includes(n)) {
    if (days.length <= 1) return; // 至少留一天，否则语义变成“每天都休息”
    plan.value.workDays = days.filter((x) => x !== n);
  } else {
    plan.value.workDays = [...days, n].sort((a, b) => a - b);
  }
}
function addRange() {
  if (!plan.value?.workScheduleEnabled) return;
  if (rangeError.value) return; // 还有非法时间段时不允许继续加
  plan.value.workRanges = [...plan.value.workRanges, { start: "14:00", end: "18:00" }];
}
function dropRange(i) {
  if (!plan.value?.workScheduleEnabled) return;
  if (plan.value.workRanges.length <= 1) return;
  plan.value.workRanges = plan.value.workRanges.filter((_, j) => j !== i);
}
async function savePlan() {
  if (!plan.value || saving.value || !dirty.value) return;
  saving.value = true;
  saveError.value = "";
  try {
    await store.patchConfig({ ...plan.value });
    savedAt.value = new Date();
  } catch (err) {
    saveError.value = err instanceof Error ? err.message : String(err);
  } finally {
    saving.value = false;
  }
}

const sum = (key) => history.value.reduce((acc, row) => acc + (row[key] ?? 0), 0);
const weekWork = computed(() => sum("workSeconds"));
const weekDone = computed(() => sum("restCompleted"));
const weekSkipped = computed(() => sum("restSkipped"));
</script>

<template>
  <div class="page">
    <!-- 只有统计卡片依赖 store.stats；下面的趋势/休息计划不依赖 -->
    <template v-if="store.stats">
      <h2>今日健康</h2>

      <div class="grid">
        <div v-for="c in cards" :key="c.label" class="stat">
          <div class="value">{{ c.value }}</div>
          <div class="label-row">
            <span class="label">{{ c.label }}</span>
            <span v-if="c.extra" class="extra">{{ c.extra }}</span>
          </div>
        </div>
      </div>
    </template>

    <section class="trend">
      <div class="trend-head">
        <div class="section-title">最近 7 天</div>
        <div v-if="streak > 0" class="streak">连续 {{ streak }} 天</div>
      </div>

      <div v-if="hasData" class="chart">
        <div v-for="row in chart" :key="row.date" class="col">
          <div class="bar-slot" :title="row.tooltip">
            <div class="bar" :class="{ today: row.isToday }" :style="{ height: row.height }" />
          </div>
          <div class="rests" :class="{ none: row.completed === 0 }">
            {{ row.completed > 0 ? row.completed : "·" }}
          </div>
          <div class="day-label" :class="{ today: row.isToday }">{{ row.label }}</div>
        </div>
      </div>
      <div v-else class="empty">
        用满一天后，这里会开始显示工作时长和休息次数的变化。
      </div>

      <div v-if="hasData" class="summary">
        本周工作 <b>{{ formatDuration(weekWork) }}</b> · 完成 <b>{{ weekDone }}</b> 次休息
        <template v-if="weekSkipped > 0"> · 跳过 <b>{{ weekSkipped }}</b> 次</template>
      </div>
      <div v-if="hasData" class="legend">柱高为当天有效工时，数字是完成的休息次数</div>
    </section>

    <section v-if="plan" class="plan">
      <div class="section-title plan-title">休息计划</div>

      <div class="plan-card">
        <div class="row between">
          <div>
            <div class="switch-title">只在工作时间段提醒</div>
            <div class="hint">段内才计时，段外倒计时清零；到点的休息不打断，自然结束</div>
          </div>
          <n-switch
            :value="plan.workScheduleEnabled"
            @update:value="(v) => (plan.workScheduleEnabled = v)"
          />
        </div>

        <!-- 时间段常显：未启用时置灰只读，避免"功能不见了" -->
        <div class="ranges" :class="{ off: !plan.workScheduleEnabled }">
          <div class="days-row">
            <span class="row-label">星期</span>
            <div class="days">
              <button
                v-for="d in DAYS"
                :key="d.n"
                type="button"
                class="day-btn"
                :class="{ on: plan.workDays.includes(d.n) }"
                :disabled="!plan.workScheduleEnabled || (plan.workDays.length <= 1 && plan.workDays.includes(d.n))"
                @click="toggleDay(d.n)"
              >
                {{ d.label }}
              </button>
            </div>
          </div>
          <div class="hint days-hint">只在勾选的星期按时段计时；没勾的星期算休息日。</div>
          <div class="divider" />
          <div v-for="(r, i) in plan.workRanges" :key="i" class="time-row">
            <label>
              开始
              <input
                type="time"
                :value="r.start"
                :disabled="!plan.workScheduleEnabled"
                @change="setRangeStart(i, $event)"
              />
            </label>
            <span>至</span>
            <label>
              结束
              <input
                type="time"
                :value="r.end"
                :disabled="!plan.workScheduleEnabled"
                @change="setRangeEnd(i, $event)"
              />
            </label>
            <button
              class="range-drop"
              type="button"
              title="删除这一段"
              :disabled="!plan.workScheduleEnabled || plan.workRanges.length <= 1"
              @click="dropRange(i)"
            >
              ×
            </button>
          </div>
          <button
            class="range-add"
            type="button"
            :disabled="!plan.workScheduleEnabled || !!rangeError"
            @click="addRange"
          >
            + 再加一段
          </button>
          <div v-if="rangeError && plan.workScheduleEnabled" class="range-error">{{ rangeError }}</div>
          <div v-if="!plan.workScheduleEnabled" class="hint range-hint">
            打开上面的开关后，休息计划只在上面勾选的星期和这些时段内计时。
          </div>
          <div v-else class="hint range-hint">
            可分上下午多段，例如 9:00-12:00、13:00-18:00。不支持跨午夜。
          </div>
        </div>

        <div class="divider" />

        <div class="row">
          <span class="row-label">工作间隔</span>
          <n-slider
            v-model:value="plan.workMinutes"
            :min="5"
            :max="180"
            :step="5"
            style="flex: 1"
          />
          <span class="row-value">{{ plan.workMinutes }} 分钟</span>
        </div>
        <div class="row">
          <span class="row-label">短休息</span>
          <n-slider
            v-model:value="plan.shortRestMinutes"
            :min="1"
            :max="30"
            :step="1"
            style="flex: 1"
          />
          <span class="row-value">{{ plan.shortRestMinutes }} 分钟</span>
        </div>
        <div class="row">
          <span class="row-label">长休触发</span>
          <n-slider
            v-model:value="plan.longRestEvery"
            :min="1"
            :max="10"
            :step="1"
            style="flex: 1"
          />
          <span class="row-value">{{ plan.longRestEvery }} 次后</span>
        </div>
        <div class="row">
          <span class="row-label">长休息</span>
          <n-slider
            v-model:value="plan.longRestMinutes"
            :min="5"
            :max="60"
            :step="5"
            style="flex: 1"
          />
          <span class="row-value">{{ plan.longRestMinutes }} 分钟</span>
        </div>
        <div class="row">
          <span class="row-label">键鼠空闲</span>
          <n-slider
            v-model:value="plan.activeIdleSecs"
            :min="10"
            :max="180"
            :step="10"
            style="flex: 1"
          />
          <span class="row-value">{{ plan.activeIdleSecs }} 秒</span>
        </div>
        <div class="row">
          <span class="row-label">离开判定</span>
          <n-slider
            v-model:value="plan.idlePauseMinutes"
            :min="1"
            :max="30"
            :step="1"
            style="flex: 1"
          />
          <span class="row-value">{{ plan.idlePauseMinutes }} 分钟</span>
        </div>
        <!-- 说明文字：三个片段必须紧贴写（模板里的空白节点会被渲染成一个空格，中文标点前多出空格） -->
        <!-- prettier-ignore -->
        <div class="hint plan-hint"><template v-if="plan.activeIdleSecs > 0">超过 <b>{{ plan.activeIdleSecs }}</b> 秒没碰鼠标键盘，就停算<b>有效工时</b>（休息倒计时也跟着停）</template><template v-else><b>键鼠门控已关闭</b>，有效工时按墙钟一直累计</template><template v-if="plan.idlePauseMinutes > 0">；离开超过 <b>{{ plan.idlePauseMinutes }}</b> 分钟，再停算<b>在座时长</b>。</template><template v-else>。</template></div>

        <div class="save-row">
          <n-button type="primary" :loading="saving" :disabled="!dirty || saving" @click="savePlan">
            {{ dirty ? "保存" : "已保存" }}
          </n-button>
          <span v-if="saveError" class="save-msg err">保存失败：{{ saveError }}</span>
          <span v-else-if="dirty" class="save-msg">有未保存的修改</span>
          <span v-else-if="savedAt" class="save-msg ok">✓ 已保存 {{ hhmm(savedAt) }}</span>
        </div>
      </div>
    </section>
  </div>
</template>
<style scoped>
/* 还原自 huyanbao.exe 内嵌 bundle 的 scoped 样式（原作用域 data-v-bece1032） */
.page h2{margin:0 0 14px;font-size:18px}
.grid{display:grid;grid-template-columns:1fr 1fr;gap:10px}
.stat{background:var(--card);border:1px solid var(--border);border-radius:14px;padding:14px 12px}
.value{font-size:18px;font-weight:700;color:var(--lav-700);font-variant-numeric:tabular-nums}
.label-row{display:flex;align-items:baseline;justify-content:space-between;gap:6px;margin-top:4px}
.label{font-size:12px;color:var(--muted)}
.extra{font-size:10px;color:var(--muted);opacity:.85;white-space:nowrap;font-variant-numeric:tabular-nums}
.trend{margin-top:18px}
.trend-head{display:flex;align-items:baseline;justify-content:space-between;margin-bottom:10px}
.section-title{font-size:15px;font-weight:700}
.streak{font-size:12px;font-weight:700;color:var(--lav-700)}
.chart{display:grid;grid-template-columns:repeat(7,1fr);gap:8px;padding:12px 10px 10px;border:1px solid var(--border);border-radius:14px;background:var(--card)}
.col{display:flex;flex-direction:column;align-items:center;gap:5px}
.bar-slot{display:flex;align-items:flex-end;width:100%;height:78px}
.bar{width:100%;border-radius:5px 5px 2px 2px;background:linear-gradient(180deg,var(--lav-500),var(--lav-700));opacity:.55;transition:height .24s ease}
.bar.today{opacity:1}
.rests{font-size:11px;font-weight:700;color:var(--lav-700);font-variant-numeric:tabular-nums}
.rests.none{color:var(--muted);font-weight:400}
.day-label{font-size:11px;color:var(--muted)}
.day-label.today{color:var(--lav-700);font-weight:700}
.summary{margin-top:10px;font-size:12px;color:var(--muted);line-height:1.6}
.summary b{color:var(--lav-700);font-variant-numeric:tabular-nums}
.empty,.legend{font-size:11px;color:var(--muted);line-height:1.6}
.empty{padding:16px 12px;border:1px dashed var(--border);border-radius:14px;text-align:center}
.legend{margin-top:4px}
/* 休息计划（从宠物页迁入） */
.plan{margin-top:18px}
.plan-title{margin-bottom:10px}
.plan-card{padding:14px 12px;border:1px solid var(--border);border-radius:14px;background:var(--card)}
.row{display:flex;align-items:center;gap:8px;margin-bottom:10px;font-size:12px}
.row.between{justify-content:space-between;align-items:flex-start;margin-bottom:12px}
.row-label{width:58px;flex:none}
.row-value{width:58px;flex:none;text-align:right;color:var(--muted);font-variant-numeric:tabular-nums}
.switch-title{font-size:13px;font-weight:700}
.hint{margin-top:3px;font-size:11px;color:var(--muted);line-height:1.5}
.plan-hint{margin:2px 0 12px;padding:8px 10px;border-radius:10px;background:var(--lav-50);color:var(--lav-700)}
.plan-hint b{font-weight:700}
.range-hint{margin:-4px 0 10px}
.ranges{transition:opacity .2s ease}
.ranges.off{opacity:.45}
.ranges.off input,.ranges.off button{cursor:not-allowed}
.days-row{display:flex;align-items:center;gap:8px;margin-bottom:4px}
.days{display:flex;gap:6px}
.days-hint{margin:0 0 10px}
.divider{height:1px;margin:12px 0;background:var(--border);opacity:.7}
.day-btn{width:28px;height:26px;padding:0;border:1px solid var(--border);border-radius:8px;background:var(--card);color:var(--muted);font-size:12px;cursor:pointer}
.day-btn.on{border-color:var(--lav-500);background:var(--lav-50);color:var(--lav-700);font-weight:700}
.day-btn:disabled{opacity:.45;cursor:default}
.time-row{display:flex;align-items:center;gap:10px;margin-bottom:8px;font-size:12px;color:var(--muted)}
.time-row input{margin-left:6px;padding:4px 8px;border:1px solid var(--border);border-radius:8px;background:var(--key-bg);color:var(--ink);font-size:12px}
.range-drop{width:24px;height:24px;padding:0;border:1px solid var(--border);border-radius:8px;background:var(--card);color:var(--muted);cursor:pointer;line-height:1}
.range-drop:disabled{opacity:.4;cursor:default}
.range-add{margin:0 0 8px;padding:4px 10px;border:1px dashed var(--border);border-radius:8px;background:transparent;color:var(--muted);font-size:12px;cursor:pointer}
.range-add:disabled{opacity:.5;cursor:default}
.range-error{margin:-4px 0 8px;font-size:11px;color:#e57373}
.save-row{display:flex;align-items:center;gap:10px;margin-top:2px}
.save-msg{font-size:11px;color:var(--muted);line-height:1.5}
.save-msg.ok{color:#43a047}
.save-msg.err{color:#e57373}
</style>
