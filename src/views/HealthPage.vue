<script setup>
import { computed, onMounted, ref } from "vue";
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

const iso = (d) =>
  `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;

/** 今日统计卡片 */
const cards = computed(() => {
  const s = store.stats;
  if (!s) return [];
  const list = [
    { label: "连续工作", value: formatDuration(s.workSeconds) },
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

const sum = (key) => history.value.reduce((acc, row) => acc + (row[key] ?? 0), 0);
const weekWork = computed(() => sum("workSeconds"));
const weekDone = computed(() => sum("restCompleted"));
const weekSkipped = computed(() => sum("restSkipped"));
</script>

<template>
  <div v-if="store.stats" class="page">
    <h2>今日健康</h2>

    <div class="grid">
      <div v-for="c in cards" :key="c.label" class="stat">
        <div class="value">{{ c.value }}</div>
        <div class="label">{{ c.label }}</div>
      </div>
    </div>

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
      <div v-if="hasData" class="legend">柱高为当天工作时长，数字是完成的休息次数</div>
    </section>
  </div>
</template>
<style scoped>
/* 还原自 huyanbao.exe 内嵌 bundle 的 scoped 样式（原作用域 data-v-bece1032） */
.page h2{margin:0 0 14px;font-size:18px}
.grid{display:grid;grid-template-columns:1fr 1fr;gap:10px}
.stat{background:var(--card);border:1px solid var(--border);border-radius:14px;padding:14px 12px}
.value{font-size:18px;font-weight:700;color:var(--mint-700)}
.label{margin-top:4px;font-size:12px;color:var(--muted)}
.trend{margin-top:18px}
.trend-head{display:flex;align-items:baseline;justify-content:space-between;margin-bottom:10px}
.section-title{font-size:15px;font-weight:700}
.streak{font-size:12px;font-weight:700;color:var(--mint-700)}
.chart{display:grid;grid-template-columns:repeat(7,1fr);gap:8px;padding:12px 10px 10px;border:1px solid var(--border);border-radius:14px;background:var(--card)}
.col{display:flex;flex-direction:column;align-items:center;gap:5px}
.bar-slot{display:flex;align-items:flex-end;width:100%;height:78px}
.bar{width:100%;border-radius:5px 5px 2px 2px;background:linear-gradient(180deg,var(--mint-500),var(--mint-700));opacity:.55;transition:height .24s ease}
.bar.today{opacity:1}
.rests{font-size:11px;font-weight:700;color:var(--mint-700);font-variant-numeric:tabular-nums}
.rests.none{color:var(--muted);font-weight:400}
.day-label{font-size:11px;color:var(--muted)}
.day-label.today{color:var(--mint-700);font-weight:700}
.summary{margin-top:10px;font-size:12px;color:var(--muted);line-height:1.6}
.summary b{color:var(--mint-700);font-variant-numeric:tabular-nums}
.empty,.legend{font-size:11px;color:var(--muted);line-height:1.6}
.empty{padding:16px 12px;border:1px dashed var(--border);border-radius:14px;text-align:center}
.legend{margin-top:4px}
</style>
