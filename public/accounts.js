const tauriInvoke =
  typeof window !== 'undefined' && typeof window.__TAURI__?.core?.invoke === 'function'
    ? window.__TAURI__.core.invoke
    : null;

const tauriListen =
  typeof window !== 'undefined' && typeof window.__TAURI__?.event?.listen === 'function'
    ? window.__TAURI__.event.listen
    : null;

const WAIT_TIMEOUT_MS = 300_000;
const ACTIVE_REFRESH_MS = 5_000;
const LOGIN_POLL_MS = 1_200;
const RESTART_PREF_KEY = 'agydock_accounts_restart';

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

function nowTimeStr() {
  const d = new Date();
  return `${pad2(d.getHours())}:${pad2(d.getMinutes())}:${pad2(d.getSeconds())}`;
}

function formatDateTime(ms) {
  if (!ms) return '—';
  const d = new Date(ms);
  return `${d.getFullYear()}-${pad2(d.getMonth() + 1)}-${pad2(d.getDate())} ${pad2(d.getHours())}:${pad2(
    d.getMinutes()
  )}`;
}

function formatShortDate(ms) {
  if (!ms) return '—';
  const d = new Date(ms);
  return `${pad2(d.getMonth() + 1)}-${pad2(d.getDate())} ${pad2(d.getHours())}:${pad2(d.getMinutes())}`;
}

function formatRelativeTime(ms) {
  if (!ms) return '—';
  const diffSec = Math.floor((Date.now() - ms) / 1000);
  if (diffSec < 60) return '刚刚';
  if (diffSec < 3600) return `${Math.floor(diffSec / 60)} 分钟前`;
  if (diffSec < 86400) return `${Math.floor(diffSec / 3600)} 小时前`;
  const days = Math.floor(diffSec / 86400);
  if (days < 30) return `${days} 天前`;
  return formatShortDate(ms);
}

function formatExpiryDelta(ms) {
  if (!ms) return '到期时间未知';
  const diffMs = ms - Date.now();
  const absMin = Math.round(Math.abs(diffMs) / 60_000);
  const label =
    absMin < 1
      ? '<1m'
      : absMin < 60
      ? `${absMin}m`
      : absMin < 24 * 60
      ? `${Math.floor(absMin / 60)}h${absMin % 60 ? ` ${absMin % 60}m` : ''}`
      : `${Math.floor(absMin / (24 * 60))}d`;
  return diffMs > 0 ? `剩余 ${label}` : `已过期 ${label}`;
}

function loadRestartPref() {
  try {
    const raw = window.localStorage.getItem(RESTART_PREF_KEY);
    return raw === null ? true : raw === '1';
  } catch {
    return true;
  }
}

function saveRestartPref(val) {
  try {
    window.localStorage.setItem(RESTART_PREF_KEY, val ? '1' : '0');
  } catch {
    // ignore storage failures
  }
}

export function initAccountsModule() {
  if (!tauriInvoke) {
    return { setActive() {} };
  }

  const els = {
    tabBtn: document.getElementById('accountsTabBtn'),
    view: document.getElementById('accountsView'),
    controls: document.getElementById('accountsControls'),
    addBtn: document.getElementById('accountAddBtn'),
    saveBtn: document.getElementById('accountSaveBtn'),
    logoutBtn: document.getElementById('accountLogoutBtn'),
    refreshBtn: document.getElementById('accountRefreshBtn'),
    detectSaveBtn: document.getElementById('accountDetectSaveBtn'),

    notice: document.getElementById('accountNotice'),
    noticeText: document.getElementById('accountNoticeText'),
    noticeCloseBtn: document.getElementById('accountNoticeCloseBtn'),
    confirmBanner: document.getElementById('accountConfirmBanner'),
    confirmText: document.getElementById('accountConfirmText'),
    confirmOkBtn: document.getElementById('accountConfirmOkBtn'),
    confirmCancelBtn: document.getElementById('accountConfirmCancelBtn'),

    waitBar: document.getElementById('accountWaitBar'),
    waitLabel: document.getElementById('accountWaitLabel'),
    waitFill: document.getElementById('accountWaitFill'),
    waitCancelBtn: document.getElementById('accountWaitCancelBtn'),
    waitStopOnlyBtn: document.getElementById('accountWaitStopOnlyBtn'),

    currentDot: document.getElementById('accCurrentDot'),
    currentEmail: document.getElementById('accCurrentEmail'),
    currentSub: document.getElementById('accCurrentSub'),
    count: document.getElementById('accCount'),
    countSub: document.getElementById('accCountSub'),
    envDot: document.getElementById('accEnvDot'),
    envValue: document.getElementById('accEnvValue'),
    envSub: document.getElementById('accEnvSub'),
    lastUsed: document.getElementById('accLastUsed'),
    lastUsedSub: document.getElementById('accLastUsedSub'),

    listNote: document.getElementById('accListNote'),
    restartChk: document.getElementById('accountRestartChk'),
    searchInput: document.getElementById('accountSearch'),
    sortSelect: document.getElementById('accountSort'),
    verifyBtn: document.getElementById('accountVerifyBtn'),
    openDirBtn: document.getElementById('accountOpenDirBtn'),
    tableBody: document.getElementById('accountsTableBody'),

    logCount: document.getElementById('accountLogCount'),
    logPreview: document.getElementById('accountLogPreview'),
    logToggleBtn: document.getElementById('accountLogToggleBtn'),
    logBody: document.getElementById('accountLogBody'),
    logPre: document.getElementById('accountLogPre'),
    logCopyBtn: document.getElementById('accountLogCopyBtn'),
    logClearBtn: document.getElementById('accountLogClearBtn'),

    footerLeft: document.getElementById('accountFooterLeft'),
    footerRight: document.getElementById('accountFooterRight'),
  };

  const state = {
    active: false,
    initialized: false,
    loading: false,
    busy: false,
    data: null,
    runtime: null,
    healthMap: new Map(),
    logs: [],
    logsExpanded: false,
    waiting: false,
    pollInFlight: false,
    waitTimer: null,
    pollTimer: null,
    waitDeadline: 0,
    previousEmail: null,
    renamingId: null,
    pendingConfirm: null,
    activeTimer: null,
    restartOnSwitch: loadRestartPref(),
    search: '',
    sort: 'default',
    unlistenTray: null,
  };

  if (els.restartChk) {
    els.restartChk.checked = state.restartOnSwitch;
  }

  function head(value) {
    return String(value ?? '').trim().toLowerCase();
  }

  const isCurrent = (email) =>
    Boolean(email) && head(state.data?.current?.email) === head(email);

  function setLogsExpanded(expanded) {
    state.logsExpanded = expanded;
    els.logBody.classList.toggle('hidden', !expanded);
    els.logToggleBtn.textContent = expanded ? '收起日志' : '展开日志';
    if (expanded) {
      els.logPre.scrollTop = els.logPre.scrollHeight;
    }
  }

  function appendLog(message) {
    let lastLine = '';
    for (const line of String(message ?? '').split(/\r?\n/)) {
      if (!line.trim()) continue;
      const formatted = `[${nowTimeStr()}] ${line}`;
      state.logs.push(formatted);
      lastLine = formatted;
    }
    if (state.logs.length > 400) state.logs = state.logs.slice(-400);
    els.logPre.textContent = state.logs.join('\n');
    els.logPre.scrollTop = els.logPre.scrollHeight;
    els.logCount.textContent = String(state.logs.length);
    if (lastLine) {
      els.logPreview.textContent = lastLine;
      els.logPreview.title = lastLine;
    }
  }

  function showNotice(text, kind = 'ok') {
    if (!text) {
      els.notice.className = 'notice hidden';
      els.noticeText.textContent = '';
      return;
    }
    const cls =
      kind === 'error'
        ? 'notice notice-error'
        : kind === 'ok'
        ? 'notice notice-ok'
        : 'notice';
    els.notice.className = cls;
    els.noticeText.textContent = text;
  }

  function showConfirm(message, onConfirm) {
    state.pendingConfirm = onConfirm;
    els.confirmText.textContent = message;
    els.confirmBanner.classList.remove('hidden');
    els.confirmBanner.scrollIntoView({ behavior: 'smooth', block: 'nearest' });
  }

  function hideConfirm() {
    state.pendingConfirm = null;
    els.confirmBanner.classList.add('hidden');
  }

  function updateTabBadge() {
    if (!els.tabBtn) return;
    els.tabBtn.textContent = state.waiting ? '账号管理 · 等待登录' : '账号管理';
  }

  function setBusy(busy) {
    state.busy = busy;
    const locked = busy || state.waiting;
    const present = Boolean(state.data?.current?.present);

    if (els.addBtn) els.addBtn.disabled = locked;
    if (els.saveBtn) els.saveBtn.disabled = locked;
    if (els.logoutBtn) els.logoutBtn.disabled = locked || !present;
    if (els.verifyBtn) els.verifyBtn.disabled = locked;
    if (els.refreshBtn) els.refreshBtn.disabled = busy;

    els.tableBody.querySelectorAll('button[data-act]').forEach((btn) => {
      if (btn.dataset.locked === '1') {
        btn.disabled = true;
      } else {
        btn.disabled = locked;
      }
    });
  }

  async function copyText(text, label) {
    if (!text) return;
    try {
      await navigator.clipboard.writeText(text);
      showNotice(`已复制${label}: ${text}`, 'ok');
    } catch {
      appendLog(`[!] 无法写入剪贴板: ${text}`);
    }
  }

  function renderKpis(data) {
    const current = data.current || {};
    const present = Boolean(current.present);
    const email = current.email || null;
    const accounts = data.accounts || [];

    // KPI 1: 当前登录账号
    els.currentEmail.textContent = email || '未登录';
    els.currentEmail.title = email ? `${email}（点击复制）` : '';
    els.currentEmail.classList.toggle('acc-copyable', Boolean(email));
    els.currentDot.className = 'status-dot';
    const currentEntry = accounts.find((a) => email && a.email === head(email));
    const remarkPrefix = currentEntry?.name ? `${currentEntry.name} · ` : '';
    if (!present) {
      els.currentDot.classList.add('muted');
      els.currentSub.textContent = '请在 Antigravity 中登录，或点「添加账号」';
      els.currentSub.title = '';
    } else if (current.hasRefreshToken) {
      els.currentSub.textContent = `${remarkPrefix}登录正常 · 含刷新令牌（支持自动续期）`;
      els.currentSub.title = current.expiresAtMs
        ? `最近签发 access_token 状态: ${formatExpiryDelta(current.expiresAtMs)}（${formatDateTime(
            current.expiresAtMs
          )}），Antigravity 运行时会按需用 refresh_token 自动续期`
        : '';
    } else if (current.expired) {
      els.currentDot.classList.add('err');
      els.currentSub.textContent = `${remarkPrefix}凭据已失效 · 缺少 refresh_token，需重新登录`;
      els.currentSub.title = '';
    } else {
      els.currentDot.classList.add('warn');
      els.currentSub.textContent = `${remarkPrefix}临时凭据 (${formatExpiryDelta(
        current.expiresAtMs
      )}) · 无刷新令牌`;
      els.currentSub.title = '';
    }

    // KPI 2: 已保存账号与备份健康
    const total = accounts.length;
    const brokenCount = accounts.filter((a) => {
      const h = state.healthMap.get(a.id);
      if (h) return h.status !== 'ok';
      return a.backupOk === false;
    }).length;
    const noRefreshCount = accounts.filter((a) => !a.hasRefreshToken).length;

    els.count.textContent = String(total);
    if (!total) {
      els.countSub.textContent = '登录过的账号会自动归档到这里';
    } else if (brokenCount > 0) {
      els.countSub.textContent = `${brokenCount} 个本地备份异常 · 建议检查或重新保存`;
    } else if (noRefreshCount > 0) {
      els.countSub.textContent = `DPAPI 加密 · ${noRefreshCount} 个缺少 refresh_token`;
    } else {
      els.countSub.textContent = 'DPAPI 加密备份 · 全部可免登录切换';
    }

    // KPI 3: 运行与代理环境
    const rt = state.runtime;
    els.envDot.className = 'status-dot';
    if (!rt) {
      els.envDot.classList.add('muted');
      els.envValue.textContent = '检测中...';
      els.envSub.textContent = '正在读取 Antigravity 与隔离代理状态';
    } else if (!rt.exeValid) {
      els.envDot.classList.add('err');
      els.envValue.textContent = '路径无效';
      els.envSub.textContent = '请在「隔离代理 → 设置」中指定 Antigravity.exe';
    } else {
      const procText =
        rt.runningCount > 0 ? `运行中 (${rt.runningCount} 进程)` : '未运行';
      els.envValue.textContent = procText;
      if (rt.proxyListening) {
        els.envSub.textContent = `隔离代理就绪 · ${rt.proxyEndpoint}`;
      } else if (rt.autoStartClash) {
        els.envDot.classList.add('warn');
        els.envSub.textContent = `代理未监听 · 重启时将尝试拉起 Clash`;
      } else {
        els.envDot.classList.add('warn');
        els.envSub.textContent = `代理未监听 · 重启时将以直连方式启动`;
      }
    }

    // KPI 4: 最近切换（全账号维度，修复此前仅查当前账号导致空白的问题）
    let latestSwitched = null;
    let totalSwitches = 0;
    for (const acc of accounts) {
      totalSwitches += acc.switchCount || 0;
      if (acc.lastUsedAtMs && (!latestSwitched || acc.lastUsedAtMs > latestSwitched.lastUsedAtMs)) {
        latestSwitched = acc;
      }
    }
    if (latestSwitched) {
      els.lastUsed.textContent = formatRelativeTime(latestSwitched.lastUsedAtMs);
      els.lastUsed.title = formatDateTime(latestSwitched.lastUsedAtMs);
      const who = latestSwitched.name || latestSwitched.email;
      els.lastUsedSub.textContent = `${who} · 累计切换 ${totalSwitches} 次`;
    } else {
      els.lastUsed.textContent = '—';
      els.lastUsed.title = '';
      els.lastUsedSub.textContent = '尚未通过本工具切换过账号';
    }

    els.footerLeft.textContent = `账号目录: ${data.storeDir || '—'} · 托盘右键可快速切换`;
    els.footerRight.textContent = `更新于 ${nowTimeStr()}`;

    const detectedUnsaved =
      present && email && !accounts.some((a) => a.email === head(email));
    els.detectSaveBtn.classList.toggle('hidden', !detectedUnsaved);
    if (detectedUnsaved) {
      els.detectSaveBtn.textContent = data.ignoredCurrent
        ? `重新归档 ${email}`
        : `归档当前账号 (${email})`;
    }
  }

  function getFilteredSortedAccounts(accounts) {
    const q = state.search.trim().toLowerCase();
    let list = accounts;
    if (q) {
      list = accounts.filter(
        (acc) =>
          acc.email.toLowerCase().includes(q) ||
          (acc.name && acc.name.toLowerCase().includes(q)) ||
          (acc.authMethod && acc.authMethod.toLowerCase().includes(q))
      );
    }

    if (state.sort === 'default') {
      return list;
    }

    const sorted = [...list];
    if (state.sort === 'recent') {
      sorted.sort((a, b) => (b.lastUsedAtMs || 0) - (a.lastUsedAtMs || 0));
    } else if (state.sort === 'switches') {
      sorted.sort(
        (a, b) =>
          (b.switchCount || 0) - (a.switchCount || 0) ||
          (b.lastUsedAtMs || 0) - (a.lastUsedAtMs || 0)
      );
    } else if (state.sort === 'saved') {
      sorted.sort((a, b) => (b.savedAtMs || 0) - (a.savedAtMs || 0));
    } else if (state.sort === 'email') {
      sorted.sort((a, b) => a.email.localeCompare(b.email));
    }
    return sorted;
  }

  function renderTable(data) {
    const allAccounts = data.accounts || [];
    const accounts = getFilteredSortedAccounts(allAccounts);

    if (!allAccounts.length) {
      els.tableBody.innerHTML = `<tr class="empty-row"><td colspan="5">${
        data.current?.present
          ? '当前登录账号尚未归档（可能曾被移除），请点击上方「重新归档」或「保存当前账号」'
          : '还没有保存的账号 · 点击右上角「添加账号」登录一次即可永久免登录切换'
      }</td></tr>`;
      els.listNote.textContent = '登录过一次的账号会保存在本地，之后可直接切换';
      return;
    }

    if (!accounts.length) {
      els.tableBody.innerHTML =
        '<tr class="empty-row"><td colspan="5">没有匹配筛选条件的账号</td></tr>';
      els.listNote.textContent = `筛选匹配 0 / ${allAccounts.length} 个账号`;
      return;
    }

    const switchLabel = state.restartOnSwitch ? '切换并重启' : '切换凭据';

    els.tableBody.innerHTML = accounts
      .map((acc) => {
        const current = isCurrent(acc.email);
        const expired = acc.expiresAtMs ? acc.expiresAtMs <= Date.now() : false;
        const health = state.healthMap.get(acc.id);
        const backupBroken = health ? health.status !== 'ok' : acc.backupOk === false;

        const stateTags = [];
        if (current) stateTags.push('<span class="tag tag-ok">当前</span>');
        if (backupBroken) {
          const reason =
            health?.status === 'corrupted'
              ? '备份损坏'
              : '备份缺失';
          stateTags.push(
            `<span class="tag tag-err" title="${escapeHtml(
              health?.detail || '本地加密凭据文件缺失或不可解密'
            )}">${reason}</span>`
          );
        } else if (!acc.hasRefreshToken) {
          if (expired) {
            stateTags.push(
              '<span class="tag tag-err" title="缺少 refresh_token 且 access_token 已过期，需重新登录">凭据失效</span>'
            );
          } else {
            stateTags.push(
              '<span class="tag tag-warn" title="缺少 refresh_token，access_token 过期后可能需要重新登录">临时凭据</span>'
            );
          }
        } else {
          const tokenHint = acc.expiresAtMs
            ? `含长期 refresh_token，可免登录切换并按需自动续期（缓存 access_token: ${formatExpiryDelta(
                acc.expiresAtMs
              )}）`
            : '含长期 refresh_token，可免登录切换并自动续期';
          stateTags.push(
            `<span class="tag ${current ? '' : 'tag-ok'}" title="${escapeHtml(
              tokenHint
            )}">长期有效</span>`
          );
        }
        if (health?.status === 'ok') {
          stateTags.push('<span class="tag" title="已通过 DPAPI 解密与结构校验">已校验</span>');
        }

        const metaParts = [];
        if (acc.authMethod) metaParts.push(acc.authMethod);
        if (acc.hasRefreshToken) {
          metaParts.push('含刷新令牌');
        } else if (acc.expiresAtMs) {
          metaParts.push(`临时令牌 ${formatExpiryDelta(acc.expiresAtMs)}`);
        }
        metaParts.push(`保存于 ${formatShortDate(acc.savedAtMs)}`);
        const metaTooltip =
          acc.hasRefreshToken && acc.expiresAtMs
            ? `缓存 access_token 签发有效期: ${formatDateTime(acc.expiresAtMs)}（${formatExpiryDelta(
                acc.expiresAtMs
              )}，运行时由 refresh_token 自动续期）`
            : '';

        const pinBadge = acc.pinned ? '<span class="tag tag-pin">置顶</span>' : '';

        const renameCell =
          state.renamingId === acc.id
            ? `<input class="text-input acc-rename-input mono" data-rename-input="${escapeHtml(
                acc.id
              )}" value="${escapeHtml(acc.name || '')}" maxlength="48" placeholder="输入备注，回车保存" />`
            : `<span class="acc-name" data-act="rename" data-id="${escapeHtml(
                acc.id
              )}" title="点击编辑备注">${
                acc.name ? escapeHtml(acc.name) : '<span class="acc-name-empty">点击添加备注</span>'
              }</span>`;

        const usagePrimary = acc.lastUsedAtMs
          ? formatRelativeTime(acc.lastUsedAtMs)
          : current
          ? '当前会话'
          : '尚未切换';
        const usageSub =
          acc.switchCount > 0
            ? `已切换 ${acc.switchCount} 次`
            : `归档 ${formatShortDate(acc.savedAtMs)}`;
        const usageTitle = acc.lastUsedAtMs
          ? `最近切换于 ${formatDateTime(acc.lastUsedAtMs)}`
          : `归档于 ${formatDateTime(acc.savedAtMs)}`;

        const switchLocked = current || backupBroken;
        const switchBtnText = current
          ? '当前账号'
          : backupBroken
          ? '备份异常'
          : switchLabel;
        const switchTitle = current
          ? '已是当前登录账号'
          : backupBroken
          ? '本地备份文件缺失或损坏，无法切换'
          : state.restartOnSwitch
          ? '关闭 Antigravity，写入该账号凭据并重新启动'
          : '仅将该账号凭据写入 Windows 凭据管理器（不重启 Antigravity）';

        return `
          <tr class="${current ? 'selected' : ''}">
            <td>
              <div class="acc-email-row">
                <span class="acc-email mono acc-copyable" data-copy-email="${escapeHtml(
                  acc.email
                )}" title="点击复制邮箱: ${escapeHtml(acc.email)}">${escapeHtml(acc.email)}</span>
                ${pinBadge}
              </div>
              <div class="acc-meta mono" title="${escapeHtml(metaTooltip)}">${escapeHtml(
                metaParts.join(' · ')
              )}</div>
            </td>
            <td>${renameCell}</td>
            <td>
              <div class="acc-usage-primary mono" title="${escapeHtml(
                usageTitle
              )}">${escapeHtml(usagePrimary)}</div>
              <div class="acc-usage-sub mono">${escapeHtml(usageSub)}</div>
            </td>
            <td><div class="acc-tags">${stateTags.join('')}</div></td>
            <td>
              <div class="acc-actions">
                <button type="button" class="btn btn-primary" data-act="switch" data-id="${escapeHtml(
                  acc.id
                )}" data-locked="${switchLocked ? '1' : '0'}" ${
                  switchLocked ? 'disabled' : ''
                } title="${escapeHtml(switchTitle)}">${switchBtnText}</button>
                <button type="button" class="btn acc-pin-btn${
                  acc.pinned ? ' pinned' : ''
                }" data-act="pin" data-id="${escapeHtml(acc.id)}" title="${
                  acc.pinned ? '取消置顶' : '置顶该账号'
                }">${acc.pinned ? '已置顶' : '置顶'}</button>
                <button type="button" class="btn" data-act="rename-btn" data-id="${escapeHtml(
                  acc.id
                )}">备注</button>
                <button type="button" class="btn btn-danger-subtle" data-act="remove" data-id="${escapeHtml(
                  acc.id
                )}">删除</button>
              </div>
            </td>
          </tr>
        `;
      })
      .join('');

    els.listNote.textContent =
      accounts.length === allAccounts.length
        ? `共 ${allAccounts.length} 个账号 · 登录过一次即可免登录切换`
        : `筛选显示 ${accounts.length} / ${allAccounts.length} 个账号`;

    if (state.renamingId) {
      const input = els.tableBody.querySelector('[data-rename-input]');
      if (input) {
        input.focus();
        input.select();
      }
    }
  }

  function render() {
    if (!state.data) return;
    renderKpis(state.data);
    renderTable(state.data);
    attachRowHandlers();
    setBusy(state.busy);
  }

  async function refreshList({ silent = false } = {}) {
    if (state.loading) return;
    state.loading = true;
    if (!silent && els.refreshBtn) {
      els.refreshBtn.textContent = '刷新中...';
    }
    try {
      const [listData, rtData] = await Promise.all([
        tauriInvoke('accounts_list'),
        tauriInvoke('accounts_runtime_status').catch(() => null),
      ]);
      state.data = listData;
      if (rtData) state.runtime = rtData;
      if (state.data?.error && !silent) {
        showNotice(state.data.error, 'warn');
      }
      render();
    } catch (err) {
      if (!silent) {
        showNotice(`读取账号列表失败: ${err?.message || err}`, 'error');
      }
    } finally {
      state.loading = false;
      if (!silent && els.refreshBtn) {
        els.refreshBtn.textContent = '刷新';
      }
    }
  }

  function startActiveRefresh() {
    stopActiveRefresh();
    state.activeTimer = setInterval(() => {
      if (
        state.active &&
        document.visibilityState === 'visible' &&
        !state.waiting &&
        !state.busy &&
        !state.renamingId &&
        !state.pendingConfirm
      ) {
        refreshList({ silent: true });
      }
    }, ACTIVE_REFRESH_MS);
  }

  function stopActiveRefresh() {
    if (state.activeTimer) {
      clearInterval(state.activeTimer);
      state.activeTimer = null;
    }
  }

  function clearWaitTimers() {
    if (state.waitTimer) {
      clearInterval(state.waitTimer);
      state.waitTimer = null;
    }
    if (state.pollTimer) {
      clearInterval(state.pollTimer);
      state.pollTimer = null;
    }
    state.pollInFlight = false;
  }

  function startWaiting() {
    state.waiting = true;
    state.pollInFlight = false;
    state.waitDeadline = Date.now() + WAIT_TIMEOUT_MS;
    els.waitBar.classList.remove('hidden');
    els.waitFill.style.width = '0%';
    updateTabBadge();

    if (state.previousEmail) {
      els.waitCancelBtn.textContent = `取消并恢复 ${state.previousEmail}`;
      els.waitStopOnlyBtn.classList.remove('hidden');
    } else {
      els.waitCancelBtn.textContent = '停止等待';
      els.waitStopOnlyBtn.classList.add('hidden');
    }
    setBusy(state.busy);

    const tick = () => {
      const elapsed = Math.min(
        WAIT_TIMEOUT_MS,
        Date.now() - (state.waitDeadline - WAIT_TIMEOUT_MS)
      );
      const total = Math.round(WAIT_TIMEOUT_MS / 1000);
      const secs = Math.min(total, Math.round(elapsed / 1000));
      els.waitLabel.textContent = `等待登录 ${secs}/${total}s`;
      els.waitFill.style.width = `${Math.round((elapsed / WAIT_TIMEOUT_MS) * 100)}%`;
      if (elapsed >= WAIT_TIMEOUT_MS) {
        stopWaiting();
        showNotice('等待登录超时：在 Antigravity 登录成功后点击「保存当前账号」即可归档。', 'warn');
        refreshList();
      }
    };
    state.waitTimer = setInterval(tick, 1000);
    tick();

    state.pollTimer = setInterval(async () => {
      if (!state.waiting || state.pollInFlight) return;
      state.pollInFlight = true;
      try {
        const cur = await tauriInvoke('accounts_current');
        if (!state.waiting) return;
        if (cur?.present && cur.email) {
          const prev = state.previousEmail;
          stopWaiting();
          const sameAsPrev = prev && head(prev) === head(cur.email);
          appendLog(
            sameAsPrev
              ? `[OK] 检测到重新登录原账号 ${cur.email}，正在更新凭据...`
              : `[OK] 检测到新登录账号 ${cur.email}，正在自动归档...`
          );
          try {
            const saved = await tauriInvoke('accounts_save_current');
            appendLog(`[OK] 已保存账号 ${saved.email}`);
            showNotice(
              sameAsPrev
                ? `已更新原账号 ${saved.email} 的登录凭据。`
                : `新账号 ${saved.email} 已自动保存，之后可一键切换。`,
              'ok'
            );
          } catch (err) {
            appendLog(`[!] 自动保存失败: ${err?.message || err}`);
            setLogsExpanded(true);
            showNotice('检测到新账号，但自动保存失败；请点「保存当前账号」重试。', 'warn');
          }
          await refreshList();
        }
      } catch (err) {
        appendLog(`[!] 轮询登录状态失败: ${err?.message || err}`);
      } finally {
        state.pollInFlight = false;
      }
    }, LOGIN_POLL_MS);
  }

  function stopWaiting() {
    state.waiting = false;
    clearWaitTimers();
    els.waitBar.classList.add('hidden');
    updateTabBadge();
    setBusy(state.busy);
  }

  async function saveCurrent() {
    setBusy(true);
    appendLog('[..] 正在保存当前登录账号...');
    try {
      const saved = await tauriInvoke('accounts_save_current');
      state.healthMap.delete(saved.id);
      appendLog(`[OK] 已保存账号 ${saved.email}`);
      showNotice(`已保存账号 ${saved.email} 的凭据备份`, 'ok');
      await refreshList();
    } catch (err) {
      appendLog(`[X] 保存失败: ${err?.message || err}`);
      setLogsExpanded(true);
      showNotice(`保存失败: ${err?.message || err}`, 'error');
    } finally {
      setBusy(false);
    }
  }

  async function switchTo(id) {
    const acc = state.data?.accounts?.find((a) => a.id === id);
    if (!acc) return;
    const label = acc.name ? `${acc.name} (${acc.email})` : acc.email;
    const msg = state.restartOnSwitch
      ? `将备份当前账号、关闭 Antigravity，把 ${label} 的登录凭据写回系统并重新启动。确认切换？`
      : `将备份当前账号，并把 ${label} 的登录凭据写入 Windows 凭据管理器（不重启 Antigravity）。确认切换？`;
    showConfirm(msg, async () => {
      hideConfirm();
      await doSwitch(id, state.restartOnSwitch);
    });
  }

  async function doSwitch(id, restart = true) {
    const acc = state.data?.accounts?.find((a) => a.id === id);
    setBusy(true);
    setLogsExpanded(true);
    appendLog(
      `[..] 正在切换到 ${acc?.email || id}${restart ? '（切换后重启）' : '（仅写入凭据）'} ...`
    );
    try {
      const res = await tauriInvoke('accounts_switch', { id, restart });
      for (const line of res.logs || []) appendLog(line);
      if (res.ok) {
        showNotice(
          restart
            ? `已切换到 ${res.email || acc?.email}，Antigravity 正在以该账号启动。`
            : `已将 ${res.email || acc?.email} 的凭据写入系统，手动重启 Antigravity 后生效。`,
          'ok'
        );
      } else {
        showNotice(`切换失败: ${res.error || '未知错误'}`, 'error');
      }
    } catch (err) {
      appendLog(`[X] 切换异常: ${err?.message || err}`);
      showNotice(`切换失败: ${err?.message || err}`, 'error');
    } finally {
      await refreshList();
      setBusy(false);
    }
  }

  async function addAccount() {
    const hasCurrent = Boolean(state.data?.current?.present);
    const currentEmail = state.data?.current?.email;
    const backupNote = hasCurrent
      ? `会先把当前账号 ${currentEmail || ''} 备份到本地，`
      : '';
    showConfirm(
      `${backupNote}然后关闭 Antigravity、清除当前登录状态并重新启动。请在重新打开的 Antigravity 中登录新账号；登录成功后会自动保存（以后可直接切换，无需再次登录）。确认继续？`,
      async () => {
        hideConfirm();
        await doBeginLogin();
      }
    );
  }

  async function doBeginLogin() {
    setBusy(true);
    setLogsExpanded(true);
    appendLog('[..] 开始添加账号：备份当前账号 → 停止 Antigravity → 清除登录状态 → 重新启动...');
    try {
      const res = await tauriInvoke('accounts_begin_login');
      for (const line of res.logs || []) appendLog(line);
      if (!res.ok) {
        showNotice(`添加账号失败: ${res.error || '未知错误'}`, 'error');
        return;
      }
      state.previousEmail = res.previousEmail || null;
      showNotice('等待登录中：请在 Antigravity 中登录新账号，登录成功后会自动保存。', 'warn');
      startWaiting();
      await refreshList();
    } catch (err) {
      appendLog(`[X] 添加账号异常: ${err?.message || err}`);
      showNotice(`添加账号失败: ${err?.message || err}`, 'error');
    } finally {
      setBusy(false);
    }
  }

  function logoutCurrent() {
    const email = state.data?.current?.email;
    if (!state.data?.current?.present) return;
    showConfirm(
      `确认退出当前账号 ${email || ''}？退出前会自动备份到本地（不会删除已保存的备份），然后关闭 Antigravity 并清除系统凭据。`,
      async () => {
        hideConfirm();
        setBusy(true);
        setLogsExpanded(true);
        appendLog(`[..] 正在退出当前账号 ${email || ''} ...`);
        try {
          const res = await tauriInvoke('accounts_logout', { stopApp: true });
          for (const line of res.logs || []) appendLog(line);
          if (res.ok) {
            showNotice(`已退出 ${res.email || '当前账号'}，本地备份已保留。`, 'ok');
          } else {
            showNotice(`退出登录失败: ${res.error || '未知错误'}`, 'error');
          }
        } catch (err) {
          appendLog(`[X] 退出异常: ${err?.message || err}`);
          showNotice(`退出登录失败: ${err?.message || err}`, 'error');
        } finally {
          await refreshList();
          setBusy(false);
        }
      }
    );
  }

  async function cancelWaiting({ restore = true } = {}) {
    stopWaiting();
    const previousEmail = state.previousEmail;
    state.previousEmail = null;
    if (!restore || !previousEmail) {
      appendLog('[i] 已停止等待登录。');
      showNotice('已停止等待登录；如需归档新账号，登录完成后点「保存当前账号」即可。', 'warn');
      return;
    }
    const acc = state.data?.accounts?.find((a) => a.email === head(previousEmail));
    if (!acc) {
      appendLog('[i] 已停止等待登录（找不到原账号的备份，未执行恢复）。');
      showNotice('已停止等待登录；如需恢复原账号请稍后手动切换。', 'warn');
      return;
    }
    appendLog(`[..] 取消添加账号，正在恢复原账号 ${acc.email} ...`);
    await doSwitch(acc.id, true);
  }

  async function togglePin(id) {
    const acc = state.data?.accounts?.find((a) => a.id === id);
    if (!acc) return;
    try {
      const updated = await tauriInvoke('accounts_toggle_pin', {
        id,
        pinned: !acc.pinned,
      });
      appendLog(
        updated.pinned
          ? `[OK] 已置顶账号 ${updated.email}`
          : `[i] 已取消置顶 ${updated.email}`
      );
      await refreshList({ silent: true });
    } catch (err) {
      appendLog(`[X] 置顶操作失败: ${err?.message || err}`);
      showNotice(`置顶操作失败: ${err?.message || err}`, 'error');
    }
  }

  async function verifyAllBackups() {
    setBusy(true);
    appendLog('[..] 正在使用 DPAPI 深度校验全部本地账号备份...');
    try {
      const results = await tauriInvoke('accounts_verify');
      state.healthMap.clear();
      let okCount = 0;
      let badCount = 0;
      for (const item of results || []) {
        state.healthMap.set(item.id, item);
        if (item.status === 'ok') {
          okCount += 1;
        } else {
          badCount += 1;
          appendLog(`[!] 备份异常 ${item.email}: ${item.detail || item.status}`);
        }
      }
      if (!results || !results.length) {
        appendLog('[i] 当前没有可校验的账号备份。');
        showNotice('当前没有已保存的账号备份。', 'warn');
      } else if (badCount === 0) {
        appendLog(`[OK] 校验完成：全部 ${okCount} 个账号备份均可正常解密。`);
        showNotice(`校验通过：全部 ${okCount} 个账号备份完好。`, 'ok');
      } else {
        setLogsExpanded(true);
        showNotice(`校验完成：${okCount} 个正常，${badCount} 个备份异常（请查看日志）。`, 'warn');
      }
      render();
    } catch (err) {
      appendLog(`[X] 备份校验失败: ${err?.message || err}`);
      setLogsExpanded(true);
      showNotice(`备份校验失败: ${err?.message || err}`, 'error');
    } finally {
      setBusy(false);
    }
  }

  async function openStoreDir() {
    try {
      const dir = await tauriInvoke('accounts_open_store_dir');
      appendLog(`[i] 已打开账号目录: ${dir}`);
    } catch (err) {
      appendLog(`[X] 无法打开账号目录: ${err?.message || err}`);
      showNotice(`打开目录失败: ${err?.message || err}`, 'error');
    }
  }

  async function renameAccount(id) {
    state.renamingId = id;
    render();
  }

  async function commitRename(id, value) {
    state.renamingId = null;
    try {
      const updated = await tauriInvoke('accounts_rename', { id, name: value });
      if (state.data) {
        const entry = state.data.accounts.find((a) => a.id === id);
        if (entry) entry.name = updated.name;
      }
      render();
    } catch (err) {
      appendLog(`[X] 保存备注失败: ${err?.message || err}`);
      showNotice(`保存备注失败: ${err?.message || err}`, 'error');
      render();
    }
  }

  function removeAccount(id) {
    const acc = state.data?.accounts?.find((a) => a.id === id);
    showConfirm(
      `确认删除账号 ${acc?.email || id} 的本地凭据备份？${
        isCurrent(acc?.email) ? '该账号当前仍处于登录状态，删除备份不会退出登录。' : ''
      }${state.data?.accounts?.length === 1 ? '（这是最后一个备份）' : ''}`,
      async () => {
        hideConfirm();
        setBusy(true);
        try {
          const removed = await tauriInvoke('accounts_remove', { id });
          state.healthMap.delete(id);
          appendLog(removed ? `[OK] 已删除账号备份 ${acc?.email || id}` : '[i] 该账号已不存在。');
          showNotice(removed ? `已删除账号备份 ${acc?.email || id}` : '该账号已不存在', 'ok');
          await refreshList();
        } catch (err) {
          appendLog(`[X] 删除失败: ${err?.message || err}`);
          showNotice(`删除失败: ${err?.message || err}`, 'error');
        } finally {
          setBusy(false);
        }
      }
    );
  }

  function attachRowHandlers() {
    if (!state._rowsWired) {
      state._rowsWired = true;
      els.tableBody.addEventListener('click', async (evt) => {
        const copyEl = evt.target.closest('[data-copy-email]');
        if (copyEl) {
          await copyText(copyEl.dataset.copyEmail, '邮箱');
          return;
        }
        const nameSpan = evt.target.closest('[data-act="rename"]');
        if (nameSpan) {
          await renameAccount(nameSpan.dataset.id);
          return;
        }
        const btn = evt.target.closest('button[data-act]');
        if (!btn || btn.disabled) return;
        const id = btn.dataset.id;
        if (btn.dataset.act === 'switch') await switchTo(id);
        else if (btn.dataset.act === 'pin') await togglePin(id);
        else if (btn.dataset.act === 'remove') removeAccount(id);
        else if (btn.dataset.act === 'rename-btn') await renameAccount(id);
      });
      els.tableBody.addEventListener('keydown', (evt) => {
        const input = evt.target.closest('[data-rename-input]');
        if (!input) return;
        if (evt.key === 'Enter') {
          evt.preventDefault();
          commitRename(input.dataset.renameInput, input.value);
        } else if (evt.key === 'Escape') {
          evt.preventDefault();
          state.renamingId = null;
          render();
        }
      });
      els.tableBody.addEventListener(
        'focusout',
        (evt) => {
          const input = evt.target.closest('[data-rename-input]');
          if (!input) return;
          if (state.renamingId === input.dataset.renameInput) {
            commitRename(input.dataset.renameInput, input.value);
          }
        },
        true
      );
    }
  }

  // Toolbar & banner bindings
  els.addBtn.addEventListener('click', addAccount);
  els.saveBtn.addEventListener('click', saveCurrent);
  els.logoutBtn.addEventListener('click', logoutCurrent);
  els.refreshBtn.addEventListener('click', () => refreshList());
  els.detectSaveBtn.addEventListener('click', saveCurrent);
  els.noticeCloseBtn.addEventListener('click', () => showNotice(''));

  els.currentEmail.addEventListener('click', () => {
    const email = state.data?.current?.email;
    if (email) copyText(email, '当前账号邮箱');
  });

  els.restartChk.addEventListener('change', () => {
    state.restartOnSwitch = Boolean(els.restartChk.checked);
    saveRestartPref(state.restartOnSwitch);
    render();
  });

  els.searchInput.addEventListener('input', () => {
    state.search = els.searchInput.value;
    if (state.data) {
      renderTable(state.data);
      setBusy(state.busy);
    }
  });

  els.sortSelect.addEventListener('change', () => {
    state.sort = els.sortSelect.value;
    if (state.data) {
      renderTable(state.data);
      setBusy(state.busy);
    }
  });

  els.verifyBtn.addEventListener('click', verifyAllBackups);
  els.openDirBtn.addEventListener('click', openStoreDir);

  els.confirmOkBtn.addEventListener('click', () => {
    const fn = state.pendingConfirm;
    hideConfirm();
    if (fn) fn();
  });
  els.confirmCancelBtn.addEventListener('click', () => {
    hideConfirm();
    appendLog('[i] 已取消操作。');
  });
  els.waitCancelBtn.addEventListener('click', () => cancelWaiting({ restore: true }));
  els.waitStopOnlyBtn.addEventListener('click', () => cancelWaiting({ restore: false }));

  els.logToggleBtn.addEventListener('click', () => setLogsExpanded(!state.logsExpanded));

  els.logCopyBtn.addEventListener('click', async () => {
    if (!state.logs.length) return;
    try {
      await navigator.clipboard.writeText(state.logs.join('\n'));
      const original = els.logCopyBtn.textContent;
      els.logCopyBtn.textContent = '已复制';
      setTimeout(() => {
        els.logCopyBtn.textContent = original;
      }, 1200);
    } catch {
      appendLog('[!] 无法写入剪贴板。');
    }
  });
  els.logClearBtn.addEventListener('click', () => {
    state.logs = [];
    els.logPre.textContent = '';
    els.logCount.textContent = '0';
    els.logPreview.textContent = '日志已清空';
  });

  async function ensureLoaded() {
    if (state.initialized) return;
    state.initialized = true;
    appendLog('[i] 就绪：凭据存于 Windows 凭据管理器，本地备份使用 DPAPI 加密，托盘右键可快速切换。');
    if (tauriListen && !state.unlistenTray) {
      try {
        state.unlistenTray = await tauriListen('accounts-changed', (event) => {
          const payload = event.payload || {};
          for (const line of payload.logs || []) appendLog(line);
          if (payload.ok) {
            showNotice(`已通过托盘切换到 ${payload.email || '目标账号'}。`, 'ok');
          } else if (payload.error) {
            setLogsExpanded(true);
            showNotice(`托盘切换失败: ${payload.error}`, 'error');
          }
          refreshList({ silent: true });
        });
      } catch {
        // ignore event registration failures
      }
    }
    await refreshList();
  }

  return {
    setActive(active) {
      state.active = active;
      if (active) {
        ensureLoaded().then(() => {
          refreshList({ silent: true });
          startActiveRefresh();
        });
      } else {
        stopActiveRefresh();
      }
    },
  };
}
