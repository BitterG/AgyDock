import {
  formatTokensM,
  formatExact,
  formatPct,
  renderCacheRateChart,
  renderTokenTrendChart,
  renderUsageHeatmap,
} from './charts.js';
import { initProxyModule } from './proxy.js';
import { initAccountsModule } from './accounts.js';
import { initUsageQuotaPanel } from './quota.js';

const tauriInvoke =
  typeof window !== 'undefined' && typeof window.__TAURI__?.core?.invoke === 'function'
    ? window.__TAURI__.core.invoke
    : null;
const isDesktopApp = Boolean(tauriInvoke);

async function apiGetUsage({ range, granularity, model, day }) {
  if (tauriInvoke) {
    return tauriInvoke('get_usage', { range, granularity, model, day: day || null });
  }
  const params = new URLSearchParams({ range, granularity, model });
  if (day) params.set('day', day);
  const res = await fetch(`/api/usage?${params.toString()}`, { cache: 'no-store' });
  if (!res.ok) throw new Error(`HTTP ${res.status}`);
  return res.json();
}

async function apiGetSessionDetail(sessionId) {
  if (tauriInvoke) {
    return tauriInvoke('get_session_detail', { sessionId });
  }
  const res = await fetch(`/api/sessions/${encodeURIComponent(sessionId)}`);
  if (!res.ok) throw new Error('读取失败');
  return res.json();
}

const state = {
  range: '7d',
  prevRange: '7d',
  selectedDay: null,
  granularity: 'day',
  granularityManual: false,
  model: 'all',
  subagentMode: 'fold',
  search: '',
  sort: 'recent',
  visibleSeries: {
    outputTokens: true,
    inputTokens: true,
    cacheReadTokens: true,
  },
  expandedSessionId: null,
  expandedChildIds: new Set(),
  sessionDetailCache: new Map(),
  data: null,
  loading: false,
};

const els = {
  brandSubStatic: document.getElementById('brandSubStatic'),
  primaryTabControl: document.getElementById('primaryTabControl'),
  usageControls: document.getElementById('usageControls'),
  proxyControls: document.getElementById('proxyControls'),
  accountsControls: document.getElementById('accountsControls'),
  usageView: document.getElementById('usageView'),
  proxyView: document.getElementById('proxyView'),
  accountsView: document.getElementById('accountsView'),

  sourceMeta: document.getElementById('sourceMeta'),
  sourceText: document.getElementById('sourceText'),
  statusDot: document.getElementById('statusDot'),
  rangeControl: document.getElementById('rangeControl'),
  dayStepper: document.getElementById('dayStepper'),
  dayPrevBtn: document.getElementById('dayPrevBtn'),
  dayPicker: document.getElementById('dayPicker'),
  dayNextBtn: document.getElementById('dayNextBtn'),
  trendTitle: document.getElementById('trendTitle'),
  trendHint: document.getElementById('trendHint'),
  exitDayBtn: document.getElementById('exitDayBtn'),
  granularityControl: document.getElementById('granularityControl'),
  modelFilter: document.getElementById('modelFilter'),
  clearModelBtn: document.getElementById('clearModelBtn'),
  modelFilterHint: document.getElementById('modelFilterHint'),
  dirBtn: document.getElementById('dirBtn'),
  exportBtn: document.getElementById('exportBtn'),
  refreshBtn: document.getElementById('refreshBtn'),
  noticeBar: document.getElementById('noticeBar'),

  kpiTotal: document.getElementById('kpiTotal'),
  kpiTotalSub: document.getElementById('kpiTotalSub'),
  kpiIo: document.getElementById('kpiIo'),
  kpiIoSub: document.getElementById('kpiIoSub'),
  kpiCache: document.getElementById('kpiCache'),
  kpiCacheSub: document.getElementById('kpiCacheSub'),
  kpiSessions: document.getElementById('kpiSessions'),
  kpiSessionsSub: document.getElementById('kpiSessionsSub'),

  seriesLegend: document.getElementById('seriesLegend'),
  heatmapStage: document.getElementById('heatmapStage'),
  heatmapSummary: document.getElementById('heatmapSummary'),
  heatmapClearDayBtn: document.getElementById('heatmapClearDayBtn'),
  trendChartStage: document.getElementById('trendChartStage'),
  trendEmpty: document.getElementById('trendEmpty'),
  cacheChartStage: document.getElementById('cacheChartStage'),
  cacheEmpty: document.getElementById('cacheEmpty'),
  cacheAvgNote: document.getElementById('cacheAvgNote'),

  modelTableBody: document.getElementById('modelTableBody'),
  sessionCountNote: document.getElementById('sessionCountNote'),
  subagentModeControl: document.getElementById('subagentModeControl'),
  sessionSearch: document.getElementById('sessionSearch'),
  sessionSort: document.getElementById('sessionSort'),
  sessionTableBody: document.getElementById('sessionTableBody'),

  footerScanInfo: document.getElementById('footerScanInfo'),
  footerUpdated: document.getElementById('footerUpdated'),
  tooltip: document.getElementById('chartTooltip'),
};

function escapeHtml(str) {
  return String(str ?? '')
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');
}

function pad2(n) {
  return String(n).padStart(2, '0');
}

function formatDateTime(ts) {
  if (!ts) return '—';
  const d = new Date(ts);
  return `${pad2(d.getMonth() + 1)}-${pad2(d.getDate())} ${pad2(d.getHours())}:${pad2(d.getMinutes())}`;
}

function formatTimeOnly(ts) {
  if (!ts) return '—';
  const d = new Date(ts);
  return `${pad2(d.getHours())}:${pad2(d.getMinutes())}:${pad2(d.getSeconds())}`;
}

function formatDuration(ms) {
  if (!ms || ms < 1000) return '<1s';
  const sec = Math.floor(ms / 1000);
  if (sec < 60) return `${sec}s`;
  const min = Math.floor(sec / 60);
  const remSec = sec % 60;
  if (min < 60) return `${min}m ${remSec}s`;
  const hr = Math.floor(min / 60);
  return `${hr}h ${min % 60}m`;
}

function formatTodayKey() {
  const d = new Date();
  return `${d.getFullYear()}-${pad2(d.getMonth() + 1)}-${pad2(d.getDate())}`;
}

function shiftDayKey(dayKey, deltaDays) {
  const m = String(dayKey || '').match(/^(\d{4})-(\d{2})-(\d{2})$/);
  const base = m
    ? new Date(Number(m[1]), Number(m[2]) - 1, Number(m[3]))
    : new Date();
  base.setDate(base.getDate() + deltaDays);
  return `${base.getFullYear()}-${pad2(base.getMonth() + 1)}-${pad2(base.getDate())}`;
}

function enterSingleDay(dayKey) {
  if (state.range !== 'day') {
    state.prevRange = state.range;
  }
  state.range = 'day';
  state.selectedDay = dayKey;
  state.granularity = 'hour';
  state.granularityManual = false;
  state.expandedSessionId = null;
  syncSegmentedButtons();
  fetchUsage();
}

function exitSingleDay(targetRange = null) {
  const nextRange = targetRange || state.prevRange || '7d';
  state.range = nextRange === 'day' ? '7d' : nextRange;
  state.selectedDay = null;
  state.granularity = state.range === '24h' ? 'hour' : 'day';
  state.granularityManual = false;
  state.expandedSessionId = null;
  syncSegmentedButtons();
  fetchUsage();
}

function syncSegmentedButtons() {
  els.rangeControl.querySelectorAll('.seg-btn').forEach((btn) => {
    btn.classList.toggle('active', btn.dataset.range === state.range);
  });
  els.granularityControl.querySelectorAll('.seg-btn').forEach((btn) => {
    btn.classList.toggle('active', btn.dataset.granularity === state.granularity);
  });
  els.subagentModeControl.querySelectorAll('.seg-btn').forEach((btn) => {
    btn.classList.toggle('active', btn.dataset.mode === state.subagentMode);
  });

  const inSingleDay = Boolean(state.selectedDay);
  els.dayStepper.classList.toggle('hidden', !inSingleDay);
  if (inSingleDay && els.dayPicker.value !== state.selectedDay) {
    els.dayPicker.value = state.selectedDay;
  }

  if (inSingleDay) {
    els.trendTitle.textContent = `Token 消耗趋势 · ${state.selectedDay}`;
    els.trendHint.classList.add('hidden');
    els.exitDayBtn.textContent = `← 返回 ${state.prevRange === 'all' ? '全部' : state.prevRange}`;
    els.exitDayBtn.classList.remove('hidden');
    els.heatmapClearDayBtn.classList.remove('hidden');
  } else {
    els.trendTitle.textContent = 'Token 消耗趋势';
    els.exitDayBtn.classList.add('hidden');
    els.heatmapClearDayBtn.classList.add('hidden');
    els.trendHint.classList.toggle('hidden', state.granularity !== 'day');
  }
}

function renderNotice(meta, summary) {
  els.noticeBar.className = 'notice hidden';
  els.noticeBar.innerHTML = '';

  if (meta.error) {
    els.noticeBar.className = 'notice notice-error';
    if (isDesktopApp) {
      els.noticeBar.innerHTML = `
        <span>${escapeHtml(meta.error)}</span>
        <button type="button" class="link-btn" id="noticePickDirBtn">选择日志目录</button>
      `;
      document.getElementById('noticePickDirBtn')?.addEventListener('click', handlePickDir);
    } else {
      els.noticeBar.textContent = meta.error;
    }
    return;
  }

  if (meta.skippedDirs > 0) {
    els.noticeBar.className = 'notice';
    els.noticeBar.textContent = `已解析 ${meta.parsedSessions} 个会话，另有 ${meta.skippedDirs} 个目录因权限限制跳过。`;
    return;
  }

  if (summary.callCount === 0 && meta.allTimeEventsCount > 0 && state.range !== 'all') {
    const scopeLabel = state.selectedDay ? `日期 ${state.selectedDay}` : `时间范围 (${state.range})`;
    els.noticeBar.className = 'notice';
    els.noticeBar.innerHTML = `
      <span>当前${escapeHtml(scopeLabel)}内没有调用记录，历史共有 ${meta.allTimeEventsCount} 条调用记录。</span>
      <button type="button" class="link-btn" id="switchAllBtn">查看全部历史</button>
    `;
    const btn = document.getElementById('switchAllBtn');
    if (btn) {
      btn.addEventListener('click', () => {
        state.selectedDay = null;
        state.range = 'all';
        state.granularity = 'day';
        state.granularityManual = false;
        syncSegmentedButtons();
        fetchUsage();
      });
    }
  }
}

function renderHeaderAndKpis(data) {
  const { meta, summary } = data;

  els.statusDot.className = 'status-dot';
  if (meta.error) {
    els.statusDot.classList.add('err');
    els.sourceText.textContent = '目录读取异常';
  } else if (meta.skippedDirs > 0) {
    els.statusDot.classList.add('warn');
    els.sourceText.textContent = `${meta.parsedSessions}/${meta.totalDirs} sessions`;
  } else {
    els.sourceText.textContent = `${meta.parsedSessions} sessions`;
  }
  els.sourceMeta.title = meta.brainDir;

  // KPI 1: Total Tokens
  els.kpiTotal.textContent = formatTokensM(summary.totalTokens);
  els.kpiTotal.title = `${formatExact(summary.totalTokens)} tokens`;
  els.kpiTotalSub.textContent = `净消耗 ${formatTokensM(summary.netTokens)} · ${summary.callCount} 次调用`;

  // KPI 2: Input / Output
  els.kpiIo.innerHTML = `${formatTokensM(summary.inputTokens)}<span class="slash">/</span>${formatTokensM(summary.outputTokens)}`;
  els.kpiIo.title = `Input: ${formatExact(summary.inputTokens)} / Output: ${formatExact(summary.outputTokens)}`;
  const outputShare = summary.netTokens > 0 ? summary.outputTokens / summary.netTokens : 0;
  els.kpiIoSub.textContent = `输出占净消耗 ${formatPct(outputShare)} · 均值 ${formatTokensM(summary.avgTokensPerCall)}/次`;

  // KPI 3: Cache Hit Rate
  els.kpiCache.textContent = summary.callCount > 0 ? formatPct(summary.cacheHitRate) : '—';
  els.kpiCache.title = `Cache Read: ${formatExact(summary.cacheReadTokens)} / Total Input: ${formatExact(summary.inputTokens + summary.cacheReadTokens)}`;
  els.kpiCacheSub.textContent = `缓存读取 ${formatTokensM(summary.cacheReadTokens)} · Fresh ${formatTokensM(summary.inputTokens)}`;

  // KPI 4: Sessions
  const mainCount = summary.userSessionCount || 0;
  const subCount = summary.subagentSessionCount || 0;
  if (mainCount > 0 && subCount > 0) {
    els.kpiSessions.textContent = String(mainCount);
    els.kpiSessions.title = `${mainCount} 个主会话 + ${subCount} 个子代理 = 共 ${summary.sessionCount} 个日志目录`;
    els.kpiSessionsSub.textContent = `含 ${subCount} 个子代理 · 最近 ${formatDateTime(summary.lastActivityAt)}`;
  } else {
    els.kpiSessions.textContent = String(summary.sessionCount);
    els.kpiSessions.title = `${summary.sessionCount} 个会话`;
    els.kpiSessionsSub.textContent =
      summary.sessionCount > 0
        ? `均耗 ${formatTokensM(summary.avgTokensPerSession)} · 最近 ${formatDateTime(summary.lastActivityAt)}`
        : '当前范围无活跃会话';
  }

  els.cacheAvgNote.textContent =
    summary.callCount > 0 ? `均值 ${formatPct(summary.cacheHitRate)}` : '均值 —';

  els.footerScanInfo.textContent = `数据源: ${meta.brainDir} (${meta.parsedSessions} 个会话)`;
  els.footerUpdated.textContent = `更新于 ${formatTimeOnly(meta.scannedAt)}`;
}

function syncModelFilterOptions(availableModels) {
  const current = state.model;
  const options = ['<option value="all">全部模型</option>'];
  for (const m of availableModels) {
    const selected = m === current ? ' selected' : '';
    options.push(`<option value="${escapeHtml(m)}"${selected}>${escapeHtml(m)}</option>`);
  }
  els.modelFilter.innerHTML = options.join('');
  if (current !== 'all' && !availableModels.includes(current)) {
    state.model = 'all';
    els.modelFilter.value = 'all';
  }

  const isFiltered = state.model !== 'all';
  els.clearModelBtn.classList.toggle('hidden', !isFiltered);
  els.modelFilterHint.textContent = isFiltered ? `已筛选: ${state.model}` : '点击行可筛选';
}

function renderModelsTable(models) {
  if (!models.length) {
    els.modelTableBody.innerHTML = '<tr class="empty-row"><td colspan="5">暂无模型调用数据</td></tr>';
    return;
  }

  els.modelTableBody.innerHTML = models
    .map((m) => {
      const selected = state.model === m.model ? ' selected' : '';
      const pct = Math.max(2, Math.round(m.share * 100));
      return `
        <tr class="clickable${selected}" data-model="${escapeHtml(m.model)}">
          <td>
            <div class="model-cell">
              <span class="model-name">${escapeHtml(m.model)}</span>
              <div class="share-track"><div class="share-fill" style="width:${pct}%"></div></div>
            </div>
          </td>
          <td class="num">${m.callCount}</td>
          <td class="num" title="${formatExact(m.netTokens)}">${formatTokensM(m.netTokens)}</td>
          <td class="num">${formatPct(m.cacheHitRate)}</td>
          <td class="num" title="${formatExact(m.totalTokens)}">${formatTokensM(m.totalTokens)}</td>
        </tr>
      `;
    })
    .join('');

  els.modelTableBody.querySelectorAll('tr[data-model]').forEach((row) => {
    row.addEventListener('click', () => {
      const clicked = row.dataset.model;
      state.model = state.model === clicked ? 'all' : clicked;
      els.modelFilter.value = state.model;
      fetchUsage();
    });
  });
}

function buildDisplaySessions(sessions) {
  if (state.subagentMode === 'flat') {
    return sessions.map((s) => ({ ...s, childSubagents: [] }));
  }

  const groupMap = new Map();
  const orphans = [];

  for (const s of sessions) {
    if (!s.isSubagent) {
      groupMap.set(s.id, {
        ...s,
        ownCallCount: s.callCount,
        ownTotalTokens: s.totalTokens,
        childSubagents: [],
      });
    }
  }

  for (const s of sessions) {
    if (!s.isSubagent) continue;
    const parentGroup = s.rootParentId ? groupMap.get(s.rootParentId) : null;
    if (!parentGroup) {
      orphans.push({ ...s, childSubagents: [] });
      continue;
    }

    parentGroup.childSubagents.push(s);
    parentGroup.callCount += s.callCount;
    parentGroup.toolCallCount += s.toolCallCount;
    parentGroup.inputTokens += s.inputTokens;
    parentGroup.outputTokens += s.outputTokens;
    parentGroup.cacheReadTokens += s.cacheReadTokens;
    parentGroup.cacheWriteTokens += s.cacheWriteTokens || 0;
    parentGroup.netTokens += s.netTokens;
    parentGroup.totalTokens += s.totalTokens;
    if (s.startedAt && (!parentGroup.startedAt || s.startedAt < parentGroup.startedAt)) {
      parentGroup.startedAt = s.startedAt;
    }
    if (s.lastAt && (!parentGroup.lastAt || s.lastAt > parentGroup.lastAt)) {
      parentGroup.lastAt = s.lastAt;
    }
    parentGroup.durationMs = Math.max(0, (parentGroup.lastAt || 0) - (parentGroup.startedAt || 0));
    const denom = parentGroup.inputTokens + parentGroup.cacheReadTokens;
    parentGroup.cacheHitRate = denom > 0 ? parentGroup.cacheReadTokens / denom : 0;
  }

  for (const g of groupMap.values()) {
    g.childSubagents.sort((a, b) => b.totalTokens - a.totalTokens);
  }

  return [...groupMap.values(), ...orphans];
}

function getFilteredSortedSessions(sessions) {
  const displayList = buildDisplaySessions(sessions);
  const q = state.search.trim().toLowerCase();
  let list = displayList;
  if (q) {
    list = displayList.filter((s) => {
      if (
        s.title.toLowerCase().includes(q) ||
        s.id.toLowerCase().includes(q) ||
        s.primaryModel.toLowerCase().includes(q) ||
        (s.parentTitle && s.parentTitle.toLowerCase().includes(q))
      ) {
        return true;
      }
      return (s.childSubagents || []).some(
        (c) =>
          c.title.toLowerCase().includes(q) ||
          c.id.toLowerCase().includes(q) ||
          c.primaryModel.toLowerCase().includes(q)
      );
    });
  }

  const sorted = [...list];
  if (state.sort === 'total') {
    sorted.sort((a, b) => b.totalTokens - a.totalTokens);
  } else if (state.sort === 'output') {
    sorted.sort((a, b) => b.outputTokens - a.outputTokens);
  } else if (state.sort === 'calls') {
    sorted.sort((a, b) => b.callCount - a.callCount);
  } else {
    sorted.sort((a, b) => (b.lastAt || 0) - (a.lastAt || 0));
  }
  return sorted;
}

async function fetchSessionDetail(sessionId) {
  let detail = state.sessionDetailCache.get(sessionId);
  if (!detail) {
    detail = await apiGetSessionDetail(sessionId);
    state.sessionDetailCache.set(sessionId, detail);
  }
  return detail;
}

function buildStepTableHtml(events) {
  const rowsHtml = events
    .map(
      (ev) => `
        <tr>
          <td class="num">#${ev.stepIndex}</td>
          <td class="mono">${formatTimeOnly(ev.ts)}</td>
          <td class="mono">${escapeHtml(ev.model)}</td>
          <td class="num" title="${formatExact(ev.inputTokens)}">${formatTokensM(ev.inputTokens)}</td>
          <td class="num" title="${formatExact(ev.outputTokens)}">${formatTokensM(ev.outputTokens)}</td>
          <td class="num" title="${formatExact(ev.cacheReadTokens)}">${formatTokensM(ev.cacheReadTokens)}</td>
          <td class="num">${ev.toolCount}</td>
          <td class="mono">${escapeHtml(ev.status)}</td>
        </tr>
      `
    )
    .join('');

  return `
    <div class="table-wrap">
      <table class="sub-table">
        <thead>
          <tr>
            <th class="num" style="width: 56px;">Step</th>
            <th style="width: 88px;">时间</th>
            <th>模型</th>
            <th class="num">Input</th>
            <th class="num">Output</th>
            <th class="num">Cache Read</th>
            <th class="num" style="width: 60px;">Tools</th>
            <th style="width: 72px;">状态</th>
          </tr>
        </thead>
        <tbody>${rowsHtml}</tbody>
      </table>
    </div>
  `;
}

async function renderSessionDetailInto(tr, session) {
  const td = tr.querySelector('td');
  td.innerHTML = '<div class="detail-box"><span class="panel-note">正在读取步骤明细...</span></div>';

  let detail;
  try {
    detail = await fetchSessionDetail(session.id);
  } catch {
    td.innerHTML = '<div class="detail-box"><span class="panel-note">无法读取会话详情</span></div>';
    return;
  }

  const children = session.childSubagents || [];
  let subagentsBlockHtml = '';
  if (children.length > 0) {
    const childRows = children
      .map((c) => {
        const typeTag = c.subagentType ? `<span class="tag">${escapeHtml(c.subagentType)}</span>` : '';
        return `
          <tr class="subagent-row" data-child-id="${escapeHtml(c.id)}">
            <td class="mono" style="white-space:nowrap;">${formatDateTime(c.lastAt)}</td>
            <td>
              <div class="session-title-row">
                ${typeTag}
                <span class="session-title" title="${escapeHtml(c.title)}">${escapeHtml(c.title)}</span>
              </div>
              <div class="session-id">${escapeHtml(c.id.slice(0, 12))}</div>
            </td>
            <td class="mono">${escapeHtml(c.primaryModel)}</td>
            <td class="num">${c.callCount}</td>
            <td class="num" title="${formatExact(c.inputTokens)}">${formatTokensM(c.inputTokens)}</td>
            <td class="num" title="${formatExact(c.outputTokens)}">${formatTokensM(c.outputTokens)}</td>
            <td class="num" title="${formatExact(c.cacheReadTokens)}">${formatTokensM(c.cacheReadTokens)}</td>
            <td class="num" title="${formatExact(c.totalTokens)}"><strong>${formatTokensM(c.totalTokens)}</strong></td>
          </tr>
          <tr class="hidden" data-child-steps-row="${escapeHtml(c.id)}">
            <td colspan="8" class="subagent-steps-cell"></td>
          </tr>
        `;
      })
      .join('');

    subagentsBlockHtml = `
      <div class="detail-section-title">归属子代理 (${children.length} 个 · 点击行可展开该子代理步骤)</div>
      <div class="table-wrap">
        <table class="sub-table">
          <thead>
            <tr>
              <th style="width: 110px;">最近时间</th>
              <th>子代理角色 / 任务</th>
              <th style="width: 170px;">主模型</th>
              <th class="num" style="width: 56px;">调用</th>
              <th class="num" style="width: 78px;">Input</th>
              <th class="num" style="width: 74px;">Output</th>
              <th class="num" style="width: 82px;">Cache</th>
              <th class="num" style="width: 82px;">总计</th>
            </tr>
          </thead>
          <tbody>${childRows}</tbody>
        </table>
      </div>
    `;
  }

  const parentNote =
    session.isSubagent && session.parentTitle
      ? ` · 归属主会话: ${session.parentTitle}`
      : '';

  td.innerHTML = `
    <div class="detail-box">
      <div class="detail-meta mono">
        <span>ID: ${escapeHtml(session.id)} · 持续 ${formatDuration(session.durationMs)} · 工具调用 ${session.toolCallCount} 次${escapeHtml(parentNote)}</span>
        <span>主会话 ${detail.events.length} 次响应${children.length ? ` · 含 ${children.length} 个子代理` : ''}</span>
      </div>
      ${subagentsBlockHtml}
      <div class="detail-section-title">${children.length ? '主会话自身步骤明细' : '步骤明细'}</div>
      ${buildStepTableHtml(detail.events)}
    </div>
  `;

  td.querySelectorAll('tr.subagent-row').forEach((childTr) => {
    childTr.addEventListener('click', async (evt) => {
      evt.stopPropagation();
      const cid = childTr.dataset.childId;
      const stepsRow = td.querySelector(`tr[data-child-steps-row="${CSS.escape(cid)}"]`);
      if (!stepsRow) return;
      const isHidden = stepsRow.classList.contains('hidden');
      if (!isHidden) {
        stepsRow.classList.add('hidden');
        return;
      }
      stepsRow.classList.remove('hidden');
      const cell = stepsRow.querySelector('td');
      cell.innerHTML = '<span class="panel-note">正在读取子代理步骤...</span>';
      try {
        const cDetail = await fetchSessionDetail(cid);
        cell.innerHTML = buildStepTableHtml(cDetail.events);
      } catch {
        cell.innerHTML = '<span class="panel-note">无法读取子代理步骤</span>';
      }
    });
  });
}

function renderSessionsTable(sessions) {
  const list = getFilteredSortedSessions(sessions);
  if (state.subagentMode === 'fold') {
    const foldedSubs = list.reduce((acc, s) => acc + (s.childSubagents?.length || 0), 0);
    els.sessionCountNote.textContent =
      foldedSubs > 0 ? `${list.length} 条主会话（已合并 ${foldedSubs} 个子代理）` : `${list.length} 条`;
  } else {
    els.sessionCountNote.textContent = `${list.length} 条（含子代理）`;
  }

  if (!list.length) {
    els.sessionTableBody.innerHTML =
      '<tr class="empty-row"><td colspan="9">当前筛选条件下无会话记录</td></tr>';
    return;
  }

  const frag = document.createDocumentFragment();
  for (const s of list) {
    const isExpanded = state.expandedSessionId === s.id;
    const tr = document.createElement('tr');
    tr.className = `clickable${isExpanded ? ' selected' : ''}`;

    const subCount = s.childSubagents?.length || 0;
    const titleTagHtml = s.isSubagent
      ? `<span class="tag">子代理${s.subagentType ? ` · ${escapeHtml(s.subagentType)}` : ''}</span>`
      : '';
    const subCountTagHtml = subCount > 0 ? `<span class="tag">+${subCount} 子代理</span>` : '';
    const parentRefHtml =
      s.isSubagent && s.parentTitle
        ? `<span class="session-parent-ref" title="归属主会话: ${escapeHtml(s.parentTitle)}">归属: ${escapeHtml(s.parentTitle)}</span>`
        : '';

    tr.innerHTML = `
      <td class="mono" style="white-space:nowrap;">${formatDateTime(s.lastAt)}</td>
      <td class="session-title-cell">
        <div class="session-title-row">
          ${titleTagHtml}
          <div class="session-title" title="${escapeHtml(s.title)}">${escapeHtml(s.title)}</div>
        </div>
        <div class="session-meta-line">
          <span class="session-id">${escapeHtml(s.id.slice(0, 12))}</span>
          ${subCountTagHtml}
          ${parentRefHtml}
        </div>
      </td>
      <td class="mono session-model-cell" title="${escapeHtml(s.primaryModel)}">${escapeHtml(s.primaryModel)}</td>
      <td class="num">${s.callCount}</td>
      <td class="num" title="${formatExact(s.inputTokens)}">${formatTokensM(s.inputTokens)}</td>
      <td class="num" title="${formatExact(s.outputTokens)}">${formatTokensM(s.outputTokens)}</td>
      <td class="num" title="${formatExact(s.cacheReadTokens)}">${formatTokensM(s.cacheReadTokens)}</td>
      <td class="num">${formatPct(s.cacheHitRate)}</td>
      <td class="num" title="${formatExact(s.totalTokens)}"><strong>${formatTokensM(s.totalTokens)}</strong></td>
    `;

    tr.addEventListener('click', () => {
      state.expandedSessionId = state.expandedSessionId === s.id ? null : s.id;
      renderSessionsTable(state.data.sessions);
    });
    frag.appendChild(tr);

    if (isExpanded) {
      const detailTr = document.createElement('tr');
      detailTr.className = 'detail-row';
      const td = document.createElement('td');
      td.colSpan = 9;
      detailTr.appendChild(td);
      frag.appendChild(detailTr);
      renderSessionDetailInto(detailTr, s);
    }
  }

  els.sessionTableBody.innerHTML = '';
  els.sessionTableBody.appendChild(frag);
}

function renderCharts() {
  if (!state.data) return;

  const cal = state.data.calendar || [];
  const calTotal = cal.reduce((acc, d) => acc + (d.totalTokens || 0), 0);
  let peakDay = null;
  for (const d of cal) {
    if (!peakDay || d.totalTokens > peakDay.totalTokens) peakDay = d;
  }
  const modelNote = state.model !== 'all' ? ` · 模型 ${state.model}` : '';
  els.heatmapSummary.textContent = peakDay
    ? `活跃 ${cal.length} 天 · 累计 ${formatTokensM(calTotal)} · 峰值 ${peakDay.day} (${formatTokensM(peakDay.totalTokens)})${modelNote}`
    : `活跃 0 天${modelNote}`;

  renderUsageHeatmap({
    container: els.heatmapStage,
    tooltipEl: els.tooltip,
    calendar: cal,
    selectedDay: state.selectedDay,
    onSelectDay: (dayKey) => {
      if (state.selectedDay === dayKey) {
        exitSingleDay();
      } else {
        enterSingleDay(dayKey);
      }
    },
  });

  const onSelectDay =
    !state.selectedDay && state.data.meta.granularity === 'day'
      ? (dayKey) => enterSingleDay(dayKey)
      : null;

  renderTokenTrendChart({
    container: els.trendChartStage,
    emptyEl: els.trendEmpty,
    tooltipEl: els.tooltip,
    series: state.data.series,
    visibleSeries: state.visibleSeries,
    onSelectDay,
  });
  renderCacheRateChart({
    container: els.cacheChartStage,
    emptyEl: els.cacheEmpty,
    tooltipEl: els.tooltip,
    series: state.data.series,
    averageRate: state.data.summary.cacheHitRate,
    onSelectDay,
  });
}

function renderAll() {
  if (!state.data) return;
  syncSegmentedButtons();
  renderNotice(state.data.meta, state.data.summary);
  renderHeaderAndKpis(state.data);
  syncModelFilterOptions(state.data.meta.availableModels);
  renderModelsTable(state.data.models);
  renderSessionsTable(state.data.sessions);
  renderCharts();
}

async function fetchUsage() {
  if (state.loading) return;
  state.loading = true;
  els.refreshBtn.textContent = '扫描中...';

  try {
    state.data = await apiGetUsage({
      range: state.range,
      granularity: state.granularity,
      model: state.model,
      day: state.selectedDay,
    });
    state.sessionDetailCache.clear();
    renderAll();
  } catch (err) {
    els.statusDot.className = 'status-dot err';
    els.sourceText.textContent = '读取失败';
    els.noticeBar.className = 'notice notice-error';
    els.noticeBar.textContent = `无法读取用量数据: ${err?.message || err}`;
  } finally {
    state.loading = false;
    els.refreshBtn.textContent = '刷新';
  }
}

async function handlePickDir(evt) {
  if (!tauriInvoke) return;
  try {
    const res = evt?.shiftKey
      ? await tauriInvoke('reset_brain_dir')
      : await tauriInvoke('pick_brain_dir');
    if (res?.changed) {
      state.expandedSessionId = null;
      await fetchUsage();
    }
  } catch (err) {
    els.noticeBar.className = 'notice notice-error';
    els.noticeBar.textContent = `切换目录失败: ${err?.message || err}`;
  }
}

// Event bindings
els.rangeControl.addEventListener('click', (evt) => {
  const btn = evt.target.closest('.seg-btn');
  if (!btn || btn.dataset.range === state.range) return;
  const next = btn.dataset.range;
  if (next === 'day') {
    const avail = state.data?.meta?.availableDays || [];
    const defaultDay = avail.length > 0 ? avail[avail.length - 1] : formatTodayKey();
    enterSingleDay(defaultDay);
    return;
  }
  state.selectedDay = null;
  state.range = next;
  state.prevRange = next;
  if (!state.granularityManual) {
    state.granularity = state.range === '24h' ? 'hour' : 'day';
  }
  syncSegmentedButtons();
  fetchUsage();
});

els.dayPicker.addEventListener('change', () => {
  if (els.dayPicker.value) {
    enterSingleDay(els.dayPicker.value);
  }
});

els.dayPrevBtn.addEventListener('click', () => {
  enterSingleDay(shiftDayKey(state.selectedDay || formatTodayKey(), -1));
});

els.dayNextBtn.addEventListener('click', () => {
  enterSingleDay(shiftDayKey(state.selectedDay || formatTodayKey(), 1));
});

els.exitDayBtn.addEventListener('click', () => {
  exitSingleDay();
});

els.heatmapClearDayBtn.addEventListener('click', () => {
  exitSingleDay();
});

els.granularityControl.addEventListener('click', (evt) => {
  const btn = evt.target.closest('.seg-btn');
  if (!btn) return;
  const nextGran = btn.dataset.granularity;
  if (state.selectedDay && nextGran === 'day') {
    exitSingleDay();
    return;
  }
  if (nextGran === state.granularity) return;
  state.granularity = nextGran;
  state.granularityManual = true;
  syncSegmentedButtons();
  fetchUsage();
});

els.modelFilter.addEventListener('change', () => {
  state.model = els.modelFilter.value;
  fetchUsage();
});

els.clearModelBtn.addEventListener('click', () => {
  state.model = 'all';
  els.modelFilter.value = 'all';
  fetchUsage();
});

els.seriesLegend.addEventListener('click', (evt) => {
  const btn = evt.target.closest('.legend-item');
  if (!btn) return;
  const key = btn.dataset.series;
  const activeCount = Object.values(state.visibleSeries).filter(Boolean).length;
  // Keep at least one series visible
  if (state.visibleSeries[key] && activeCount <= 1) return;
  state.visibleSeries[key] = !state.visibleSeries[key];
  btn.classList.toggle('active', state.visibleSeries[key]);
  renderCharts();
});

els.subagentModeControl.addEventListener('click', (evt) => {
  const btn = evt.target.closest('.seg-btn');
  if (!btn || btn.dataset.mode === state.subagentMode) return;
  state.subagentMode = btn.dataset.mode;
  state.expandedSessionId = null;
  syncSegmentedButtons();
  if (state.data) renderSessionsTable(state.data.sessions);
});

els.sessionSearch.addEventListener('input', () => {
  state.search = els.sessionSearch.value;
  if (state.data) renderSessionsTable(state.data.sessions);
});

els.sessionSort.addEventListener('change', () => {
  state.sort = els.sessionSort.value;
  if (state.data) renderSessionsTable(state.data.sessions);
});

els.refreshBtn.addEventListener('click', () => fetchUsage());

if (isDesktopApp && els.dirBtn) {
  els.dirBtn.classList.remove('hidden');
  els.dirBtn.addEventListener('click', handlePickDir);
}

els.exportBtn.addEventListener('click', async () => {
  if (tauriInvoke) {
    const origText = els.exportBtn.textContent;
    try {
      const res = await tauriInvoke('export_csv', {
        range: state.range,
        granularity: state.granularity,
        model: state.model,
        day: state.selectedDay || null,
      });
      if (res?.saved) {
        els.exportBtn.textContent = '已导出';
        setTimeout(() => {
          els.exportBtn.textContent = origText;
        }, 1600);
      }
    } catch (err) {
      els.noticeBar.className = 'notice notice-error';
      els.noticeBar.textContent = `导出 CSV 失败: ${err?.message || err}`;
    }
    return;
  }

  const params = new URLSearchParams({
    range: state.range,
    granularity: state.granularity,
    model: state.model,
  });
  if (state.selectedDay) params.set('day', state.selectedDay);
  window.location.href = `/api/export.csv?${params.toString()}`;
});

// Responsive redraw on resize
let resizeTimer = null;
const resizeObserver = new ResizeObserver(() => {
  clearTimeout(resizeTimer);
  resizeTimer = setTimeout(() => renderCharts(), 60);
});
resizeObserver.observe(els.heatmapStage);
resizeObserver.observe(els.trendChartStage);
resizeObserver.observe(els.cacheChartStage);

// Desktop module switcher (用量看板 | 隔离代理 | 账号)
const proxyModule = initProxyModule();
const accountsModule = initAccountsModule();
const quotaPanel = initUsageQuotaPanel();
const previewTab = new URLSearchParams(window.location.search).get('preview');
const accountTabAllowed = isDesktopApp;
const tabBarVisible = isDesktopApp || previewTab === 'proxy';

let activePrimaryTab = 'usage';

function switchPrimaryTab(tab) {
  const next =
    tab === 'accounts' && accountTabAllowed ? 'accounts' : tab === 'proxy' ? 'proxy' : 'usage';
  activePrimaryTab = next;
  els.primaryTabControl.querySelectorAll('.seg-btn').forEach((btn) => {
    btn.classList.toggle('active', btn.dataset.tab === next);
  });

  const isUsage = next === 'usage';
  els.usageView.classList.toggle('hidden', !isUsage);
  els.usageControls.classList.toggle('hidden', !isUsage);
  els.sourceMeta.classList.toggle('hidden', !isUsage);
  els.proxyView.classList.toggle('hidden', next !== 'proxy');
  els.proxyControls.classList.toggle('hidden', next !== 'proxy');
  els.accountsView.classList.toggle('hidden', next !== 'accounts');
  els.accountsControls.classList.toggle('hidden', next !== 'accounts');

  proxyModule.setActive(next === 'proxy');
  accountsModule.setActive(next === 'accounts');
  quotaPanel.setActive(isUsage);
  if (isUsage) {
    renderCharts();
  }
}

if (tabBarVisible) {
  els.brandSubStatic.classList.add('hidden');
  els.primaryTabControl.classList.remove('hidden');
  if (!accountTabAllowed) {
    els.primaryTabControl
      .querySelector('button[data-tab="accounts"]')
      ?.classList.add('hidden');
  }
  els.primaryTabControl.addEventListener('click', (evt) => {
    const btn = evt.target.closest('.seg-btn');
    if (!btn || btn.dataset.tab === activePrimaryTab) return;
    switchPrimaryTab(btn.dataset.tab);
  });
  if (previewTab === 'proxy') {
    switchPrimaryTab('proxy');
  } else if (previewTab === 'accounts' && accountTabAllowed) {
    switchPrimaryTab('accounts');
  }
}

// Initial load + gentle 30s auto-refresh when tab is visible
fetchUsage();
setInterval(() => {
  if (document.visibilityState === 'visible' && activePrimaryTab === 'usage') {
    fetchUsage();
  }
}, 30_000);
