const tauriInvoke =
  typeof window !== 'undefined' && typeof window.__TAURI__?.core?.invoke === 'function'
    ? window.__TAURI__.core.invoke
    : null;

const tauriListen =
  typeof window !== 'undefined' && typeof window.__TAURI__?.event?.listen === 'function'
    ? window.__TAURI__.event.listen
    : null;

function escapeHtml(str) {
  return String(str ?? '')
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');
}

function nowTimeStr() {
  const d = new Date();
  const p = (n) => String(n).padStart(2, '0');
  return `${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}`;
}

export function initProxyModule() {
  const els = {
    launchBtn: document.getElementById('proxyLaunchBtn'),
    auditBtn: document.getElementById('proxyAuditBtn'),
    settingsBtn: document.getElementById('proxySettingsBtn'),
    refreshBtn: document.getElementById('proxyRefreshBtn'),

    warnBanner: document.getElementById('proxyWarnBanner'),
    globalWarnBanner: document.getElementById('proxyGlobalWarnBanner'),
    confirmBanner: document.getElementById('proxyConfirmBanner'),
    confirmText: document.getElementById('proxyConfirmText'),
    confirmOkBtn: document.getElementById('proxyConfirmOkBtn'),
    confirmCancelBtn: document.getElementById('proxyConfirmCancelBtn'),

    auditBar: document.getElementById('proxyAuditBar'),
    auditFill: document.getElementById('proxyAuditFill'),
    auditLabel: document.getElementById('proxyAuditLabel'),

    statusDot: document.getElementById('proxyStatusDot'),
    headline: document.getElementById('proxyHeadline'),
    headlineSub: document.getElementById('proxyHeadlineSub'),
    envValue: document.getElementById('proxyEnvValue'),
    envSub: document.getElementById('proxyEnvSub'),
    procValue: document.getElementById('proxyProcValue'),
    procSub: document.getElementById('proxyProcSub'),
    connValue: document.getElementById('proxyConnValue'),
    connSub: document.getElementById('proxyConnSub'),

    settingsPanel: document.getElementById('proxySettingsPanel'),
    inputHost: document.getElementById('proxyInputHost'),
    chkAutoPort: document.getElementById('proxyChkAutoPort'),
    inputPort: document.getElementById('proxyInputPort'),
    chkBypassChina: document.getElementById('proxyChkBypassChina'),
    inputCustomBypass: document.getElementById('proxyInputCustomBypass'),
    btnClearBypass: document.getElementById('proxyBtnClearBypass'),
    chkAutoClash: document.getElementById('proxyChkAutoClash'),
    chkDisableQuic: document.getElementById('proxyChkDisableQuic'),
    chkAutoRefresh: document.getElementById('proxyChkAutoRefresh'),
    inputAgyPath: document.getElementById('proxyInputAgyPath'),
    btnBrowseAgy: document.getElementById('proxyBtnBrowseAgy'),
    inputClashPath: document.getElementById('proxyInputClashPath'),
    btnBrowseClash: document.getElementById('proxyBtnBrowseClash'),
    inputWorkspace: document.getElementById('proxyInputWorkspace'),
    btnBrowseWorkspace: document.getElementById('proxyBtnBrowseWorkspace'),
    btnClearWorkspace: document.getElementById('proxyBtnClearWorkspace'),

    subViewControl: document.getElementById('proxySubViewControl'),
    tabConnBtn: document.getElementById('proxyTabConnBtn'),
    chkShowIpc: document.getElementById('proxyChkShowIpc'),
    connTools: document.getElementById('proxyConnTools'),
    logTools: document.getElementById('proxyLogTools'),

    btnStartClash: document.getElementById('proxyBtnStartClash'),
    btnFlushDns: document.getElementById('proxyBtnFlushDns'),
    btnDryRun: document.getElementById('proxyBtnDryRun'),
    btnStopAgy: document.getElementById('proxyBtnStopAgy'),
    btnOpenReport: document.getElementById('proxyBtnOpenReport'),
    btnCopyLog: document.getElementById('proxyBtnCopyLog'),
    btnClearLog: document.getElementById('proxyBtnClearLog'),

    connView: document.getElementById('proxyConnView'),
    connTableBody: document.getElementById('proxyConnTableBody'),
    logView: document.getElementById('proxyLogView'),
    logPre: document.getElementById('proxyLogPre'),

    footerLeft: document.getElementById('proxyFooterLeft'),
    footerRight: document.getElementById('proxyFooterRight'),
  };

  const state = {
    active: false,
    subView: 'connections',
    settingsExpanded: false,
    busy: false,
    auditing: false,
    refreshing: false,
    autoRefresh: true,
    settings: null,
    snapshot: null,
    displayedConnections: [],
    logs: [],
    pendingConfirmAction: null,
    timerId: null,
    unlistenAudit: null,
  };

  function appendLog(msg) {
    const lines = String(msg ?? '').split(/\r?\n/);
    const ts = nowTimeStr();
    for (const line of lines) {
      if (!line.trim()) continue;
      state.logs.push(`[${ts}] ${line}`);
    }
    if (state.logs.length > 400) {
      state.logs = state.logs.slice(-400);
    }
    els.logPre.textContent = state.logs.join('\n');
    els.logPre.scrollTop = els.logPre.scrollHeight;
  }

  function showConfirm(message, onConfirm) {
    state.pendingConfirmAction = onConfirm;
    els.confirmText.textContent = message;
    els.confirmBanner.classList.remove('hidden');
  }

  function hideConfirm() {
    state.pendingConfirmAction = null;
    els.confirmBanner.classList.add('hidden');
  }

  function setBusy(busy) {
    state.busy = busy;
    const disabled = busy || state.auditing;
    els.launchBtn.disabled = disabled;
    els.refreshBtn.disabled = disabled;
    els.btnStartClash.disabled = disabled;
    els.btnFlushDns.disabled = disabled;
    els.btnDryRun.disabled = disabled;
    els.btnStopAgy.disabled = disabled;
  }

  function setSubView(view) {
    state.subView = view;
    els.subViewControl.querySelectorAll('.seg-btn').forEach((b) => {
      b.classList.toggle('active', b.dataset.subview === view);
    });
    const isConn = view === 'connections';
    els.connView.classList.toggle('hidden', !isConn);
    els.connTools.classList.toggle('hidden', !isConn);
    els.logView.classList.toggle('hidden', isConn);
    els.logTools.classList.toggle('hidden', isConn);
  }

  function renderConnections() {
    const showIpc = Boolean(els.chkShowIpc.checked);
    const filtered = showIpc
      ? state.displayedConnections
      : state.displayedConnections.filter((c) => c.category !== 'localIpc');

    els.tabConnBtn.textContent = `连接明细 (${filtered.length})`;

    if (!filtered.length) {
      els.connTableBody.innerHTML =
        '<tr class="empty-row"><td colspan="6">当前没有可显示的外部连接（在 Antigravity 发一条消息即可看到实时套接字）</td></tr>';
      return;
    }

    els.connTableBody.innerHTML = filtered
      .map((c) => {
        const badgeClass =
          c.category === 'proxied'
            ? 'tag tag-ok'
            : c.category === 'tunFakeIp'
              ? 'tag tag-warn'
              : c.category === 'publicDirect'
                ? 'tag tag-err'
                : 'tag';
        return `
          <tr>
            <td><span class="${badgeClass}">${escapeHtml(c.categoryLabel)}</span></td>
            <td class="mono">${escapeHtml(c.processName)}</td>
            <td class="num">${c.pid}</td>
            <td class="mono">${escapeHtml(c.localEndpoint)}</td>
            <td class="mono">${escapeHtml(c.remoteEndpoint)}</td>
            <td class="mono">${escapeHtml(c.state)} · ${escapeHtml(c.seenAt)}</td>
          </tr>
        `;
      })
      .join('');
  }

  function applySnapshot(snap, overrideConnections = null, overrideCounts = null) {
    if (!snap) return;
    state.snapshot = snap;
    state.displayedConnections = overrideConnections || snap.connections || [];

    const proxied = overrideCounts ? overrideCounts.proxied : snap.proxiedCount;
    const tun = overrideCounts ? overrideCounts.tun : snap.tunCount;
    const direct = overrideCounts ? overrideCounts.direct : snap.directCount;
    const totalProcs = (snap.antigravityProcCount || 0) + (snap.languageServerProcCount || 0);

    const bypassEnabled = Boolean(
      state.settings?.bypassChinaAndLan || state.settings?.customBypassDomains?.trim()
    );
    const lsDirect = state.displayedConnections.some(
      (c) =>
        c.category === 'publicDirect' &&
        String(c.processName || '')
          .toLowerCase()
          .startsWith('language_server')
    );

    // Status headline & dot
    els.statusDot.className = 'status-dot';
    if (!snap.isProxyListening) {
      els.statusDot.classList.add('err');
      els.headline.textContent = '代理端口未监听';
    } else if (totalProcs === 0) {
      els.statusDot.classList.add('muted');
      els.headline.textContent = 'Antigravity 未运行';
    } else if (lsDirect || (!bypassEnabled && direct > 0)) {
      els.statusDot.classList.add('err');
      els.headline.textContent = '检测到公网直连';
    } else if (tun > 0) {
      els.statusDot.classList.add('warn');
      els.headline.textContent = '由 TUN 网卡接管';
    } else if (proxied > 0 && direct > 0 && bypassEnabled) {
      els.headline.textContent = '隔离代理运行中 (含直连绕过)';
    } else if (proxied > 0) {
      els.headline.textContent = '隔离代理运行中';
    } else {
      els.headline.textContent = '运行中（空闲）';
    }

    const modeSuffix = snap.clashMode && snap.clashMode !== '未知' ? ` (${snap.clashMode})` : '';
    const bypassTag = bypassEnabled ? ' · 国内直连' : '';
    els.headlineSub.textContent = `${snap.proxyHost}:${snap.resolvedPort}${modeSuffix}${bypassTag} · ${snap.portSource}`;

    els.launchBtn.textContent = totalProcs > 0 ? '重启 Antigravity' : '启动 Antigravity';

    // System env cell
    const sysText = snap.systemProxyEnabled ? '系统代理 开' : '系统代理 关';
    const tunText = snap.tunEnabled ? 'TUN 开' : 'TUN 关';
    els.envValue.textContent = `${sysText} / ${tunText}`;
    if (snap.systemProxyEnabled || snap.tunEnabled) {
      els.envSub.textContent = snap.tunEnabled
        ? `网卡: ${snap.tunAdapterText || 'TUN Active'}`
        : `系统代理: ${snap.systemProxyServer || '已开启'}`;
    } else {
      els.envSub.textContent = '不改系统代理，仅隔离 Antigravity';
    }

    els.warnBanner.classList.toggle('hidden', !(snap.systemProxyEnabled || snap.tunEnabled));
    if (els.globalWarnBanner) {
      const isGlobalMode = String(snap.clashMode || '').trim().toLowerCase() === 'global';
      els.globalWarnBanner.classList.toggle('hidden', !isGlobalMode);
    }

    // Process cell
    els.procValue.textContent = `${totalProcs} 进程`;
    els.procSub.textContent = `App x${snap.antigravityProcCount} · LS x${snap.languageServerProcCount} · Clash ${snap.isClashRunning ? '运行中' : '未运行'}`;

    // Connections count cell
    els.connValue.innerHTML = `
      <span class="${proxied > 0 ? 'tone-ok' : 'tone-muted'}">${proxied}</span><span class="slash">/</span><span class="${tun > 0 ? 'tone-warn' : 'tone-muted'}">${tun}</span><span class="slash">/</span><span class="${direct > 0 ? 'tone-err' : 'tone-muted'}">${direct}</span>
    `;
    els.connSub.textContent = '走代理 / TUN / 公网直连';

    renderConnections();

    const ws = state.settings?.workspacePath?.trim();
    els.footerLeft.textContent = `端口来源: ${snap.portSource}${ws ? ` · 启动目录: ${ws}` : ''}`;
    els.footerRight.textContent = `更新于 ${snap.capturedAt}`;
  }

  function applySettingsToForm(s) {
    state.settings = s;
    els.inputHost.value = s.proxyHost || '127.0.0.1';
    els.chkAutoPort.checked = Boolean(s.autoDetectPort);
    els.inputPort.value = String(s.customPort || 7897);
    els.inputPort.disabled = Boolean(s.autoDetectPort);
    if (els.chkBypassChina) els.chkBypassChina.checked = s.bypassChinaAndLan !== false;
    if (els.inputCustomBypass) els.inputCustomBypass.value = s.customBypassDomains || '';
    els.chkAutoClash.checked = Boolean(s.autoStartClash);
    els.chkDisableQuic.checked = Boolean(s.disableQuic);
    els.chkShowIpc.checked = Boolean(s.showLoopbackIpc);
    els.inputAgyPath.value = s.antigravityPath || '';
    els.inputClashPath.value = s.clashVergePath || '';
    els.inputWorkspace.value = s.workspacePath || '';
  }

  function collectSettingsFromForm() {
    const portNum = parseInt(els.inputPort.value.trim(), 10);
    return {
      proxyHost: els.inputHost.value.trim() || '127.0.0.1',
      autoDetectPort: Boolean(els.chkAutoPort.checked),
      customPort: Number.isFinite(portNum) && portNum > 0 && portNum <= 65535 ? portNum : 7897,
      antigravityPath: els.inputAgyPath.value.trim(),
      clashVergePath: els.inputClashPath.value.trim(),
      workspacePath: els.inputWorkspace.value.trim(),
      autoStartClash: Boolean(els.chkAutoClash.checked),
      disableQuic: Boolean(els.chkDisableQuic.checked),
      showLoopbackIpc: Boolean(els.chkShowIpc.checked),
      bypassChinaAndLan: els.chkBypassChina ? Boolean(els.chkBypassChina.checked) : true,
      customBypassDomains: els.inputCustomBypass ? els.inputCustomBypass.value.trim() : '',
    };
  }

  let saveTimer = null;
  async function persistSettings(immediate = false) {
    if (!tauriInvoke) return;
    const doSave = async () => {
      try {
        const next = collectSettingsFromForm();
        const saved = await tauriInvoke('proxy_save_settings', { settings: next });
        state.settings = saved;
      } catch (err) {
        appendLog(`[!] 保存设置失败: ${err}`);
      }
    };
    if (immediate) {
      clearTimeout(saveTimer);
      await doSave();
    } else {
      clearTimeout(saveTimer);
      saveTimer = setTimeout(doSave, 250);
    }
  }

  async function refreshSnapshot() {
    if (!tauriInvoke || state.refreshing || state.auditing) return;
    state.refreshing = true;
    try {
      const snap = await tauriInvoke('proxy_get_snapshot');
      applySnapshot(snap);
    } catch (err) {
      appendLog(`[!] 刷新状态失败: ${err}`);
    } finally {
      state.refreshing = false;
    }
  }

  function startAutoRefresh() {
    stopAutoRefresh();
    if (!state.active || !state.autoRefresh || !tauriInvoke) return;
    state.timerId = setInterval(() => {
      if (state.active && document.visibilityState === 'visible' && !state.auditing) {
        refreshSnapshot();
      }
    }, 1500);
  }

  function stopAutoRefresh() {
    if (state.timerId) {
      clearInterval(state.timerId);
      state.timerId = null;
    }
  }

  async function runLaunch(forceRestart) {
    if (!tauriInvoke) return;
    hideConfirm();
    await persistSettings(true);
    setBusy(true);
    try {
      const res = await tauriInvoke('proxy_launch_antigravity', { forceRestart });
      for (const l of res.logs || []) appendLog(l);
      if (res.needConfirm) {
        showConfirm(
          `检测到当前有 ${res.processCount} 个 Antigravity / language_server 进程正在运行。必须先关闭旧进程才能让隔离代理环境变量生效，确认关闭并重启？`,
          () => runLaunch(true)
        );
        return;
      }
      if (!res.ok && res.error) {
        appendLog(`[X] ${res.error}`);
        setSubView('logs');
      } else if (res.ok) {
        setTimeout(() => refreshSnapshot(), 1100);
      }
    } catch (err) {
      appendLog(`[X] 启动异常: ${err}`);
      setSubView('logs');
    } finally {
      setBusy(false);
    }
  }

  async function runAudit() {
    if (!tauriInvoke) return;
    if (state.auditing) {
      await tauriInvoke('proxy_cancel_audit');
      return;
    }
    if (state.busy) return;
    await persistSettings(true);

    state.auditing = true;
    setBusy(true);
    els.auditBtn.textContent = '取消验收';
    els.auditBar.classList.remove('hidden');
    els.auditFill.style.width = '0%';
    els.auditLabel.textContent = '验收中 0/20s';
    appendLog('[..] 开始 20 秒累计验收检查（请在 Antigravity 中发一条消息）...');

    try {
      const res = await tauriInvoke('proxy_run_audit', { seconds: 20 });
      if (res.cancelled) {
        appendLog('[i] 已取消本次 20 秒验收检查。');
      } else {
        applySnapshot(res.latestSnapshot, res.cumulativeConnections, {
          proxied: res.cumulativeProxiedCount,
          tun: res.cumulativeTunCount,
          direct: res.cumulativeDirectCount,
        });
        appendLog(res.reportText);
        if (res.reportPath) {
          appendLog(`[i] 验收报告已保存: ${res.reportPath}`);
        }
        setSubView('logs');
      }
    } catch (err) {
      appendLog(`[X] 验收失败: ${err}`);
      setSubView('logs');
    } finally {
      state.auditing = false;
      els.auditBtn.textContent = '验收 20s';
      els.auditBar.classList.add('hidden');
      setBusy(false);
    }
  }

  // Event bindings
  els.launchBtn.addEventListener('click', () => runLaunch(false));
  els.auditBtn.addEventListener('click', () => runAudit());
  els.refreshBtn.addEventListener('click', () => refreshSnapshot());
  els.settingsBtn.addEventListener('click', () => {
    state.settingsExpanded = !state.settingsExpanded;
    els.settingsPanel.classList.toggle('hidden', !state.settingsExpanded);
    els.settingsBtn.textContent = state.settingsExpanded ? '收起设置' : '设置';
  });

  els.confirmOkBtn.addEventListener('click', () => {
    const fn = state.pendingConfirmAction;
    hideConfirm();
    if (fn) fn();
  });
  els.confirmCancelBtn.addEventListener('click', () => {
    hideConfirm();
    appendLog('[i] 已取消操作。');
  });

  els.subViewControl.addEventListener('click', (evt) => {
    const btn = evt.target.closest('.seg-btn');
    if (!btn) return;
    setSubView(btn.dataset.subview);
  });

  els.chkShowIpc.addEventListener('change', () => {
    persistSettings(false);
    renderConnections();
  });

  els.chkAutoPort.addEventListener('change', async () => {
    els.inputPort.disabled = Boolean(els.chkAutoPort.checked);
    await persistSettings(true);
    refreshSnapshot();
  });

  els.chkAutoRefresh.addEventListener('change', () => {
    state.autoRefresh = Boolean(els.chkAutoRefresh.checked);
    if (state.autoRefresh) startAutoRefresh();
    else stopAutoRefresh();
  });

  [
    els.inputHost,
    els.inputPort,
    els.inputAgyPath,
    els.inputClashPath,
    els.inputWorkspace,
    els.inputCustomBypass,
  ]
    .filter(Boolean)
    .forEach((input) => input.addEventListener('input', () => persistSettings(false)));
  [els.chkAutoClash, els.chkDisableQuic, els.chkBypassChina]
    .filter(Boolean)
    .forEach((chk) => chk.addEventListener('change', () => persistSettings(false)));
  if (els.btnClearBypass) {
    els.btnClearBypass.addEventListener('click', async () => {
      els.inputCustomBypass.value = '';
      await persistSettings(true);
    });
  }

  async function browseField(kind, inputEl) {
    if (!tauriInvoke) return;
    try {
      const res = await tauriInvoke('proxy_browse_path', { kind, current: inputEl.value });
      if (res?.selected) {
        inputEl.value = res.path;
        await persistSettings(true);
        refreshSnapshot();
      }
    } catch (err) {
      appendLog(`[!] 选择路径失败: ${err}`);
    }
  }

  els.btnBrowseAgy.addEventListener('click', () => browseField('antigravity', els.inputAgyPath));
  els.btnBrowseClash.addEventListener('click', () => browseField('clash', els.inputClashPath));
  els.btnBrowseWorkspace.addEventListener('click', () =>
    browseField('workspace', els.inputWorkspace)
  );
  els.btnClearWorkspace.addEventListener('click', async () => {
    els.inputWorkspace.value = '';
    await persistSettings(true);
    refreshSnapshot();
  });

  els.btnStartClash.addEventListener('click', async () => {
    if (!tauriInvoke || state.busy) return;
    await persistSettings(true);
    setBusy(true);
    try {
      const res = await tauriInvoke('proxy_start_clash');
      for (const l of res.logs || []) appendLog(l);
      await refreshSnapshot();
    } catch (err) {
      appendLog(`[X] 启动 Clash 失败: ${err}`);
    } finally {
      setBusy(false);
    }
  });

  els.btnFlushDns.addEventListener('click', async () => {
    if (!tauriInvoke || state.busy) return;
    setBusy(true);
    try {
      const res = await tauriInvoke('proxy_flush_dns');
      for (const l of res.logs || []) appendLog(l);
      setSubView('logs');
    } catch (err) {
      appendLog(`[X] 刷新 DNS 失败: ${err}`);
    } finally {
      setBusy(false);
    }
  });

  els.btnDryRun.addEventListener('click', async () => {
    if (!tauriInvoke) return;
    await persistSettings(true);
    try {
      const text = await tauriInvoke('proxy_dry_run');
      appendLog(text);
      setSubView('logs');
    } catch (err) {
      appendLog(`[X] 生成预览失败: ${err}`);
    }
  });

  els.btnStopAgy.addEventListener('click', async () => {
    if (!tauriInvoke || state.busy) return;
    const totalProcs =
      (state.snapshot?.antigravityProcCount || 0) + (state.snapshot?.languageServerProcCount || 0);
    if (totalProcs === 0) {
      appendLog('[i] 当前没有正在运行的 Antigravity 进程。');
      await refreshSnapshot();
      return;
    }
    showConfirm(`确定要结束当前正在运行的 ${totalProcs} 个 Antigravity / language_server 进程吗？`, async () => {
      setBusy(true);
      try {
        const res = await tauriInvoke('proxy_stop_antigravity');
        for (const l of res.logs || []) appendLog(l);
        await refreshSnapshot();
      } catch (err) {
        appendLog(`[X] 结束进程失败: ${err}`);
      } finally {
        setBusy(false);
      }
    });
  });

  els.btnOpenReport.addEventListener('click', async () => {
    if (!tauriInvoke) return;
    try {
      await tauriInvoke('proxy_open_report');
    } catch (err) {
      appendLog(`[!] ${err}`);
    }
  });

  els.btnCopyLog.addEventListener('click', async () => {
    if (!state.logs.length) return;
    try {
      await navigator.clipboard.writeText(state.logs.join('\n'));
      const orig = els.btnCopyLog.textContent;
      els.btnCopyLog.textContent = '已复制';
      setTimeout(() => {
        els.btnCopyLog.textContent = orig;
      }, 1200);
    } catch {
      appendLog('[!] 无法写入剪贴板。');
    }
  });

  els.btnClearLog.addEventListener('click', () => {
    state.logs = [];
    els.logPre.textContent = '';
  });

  let initialized = false;
  async function ensureLoaded() {
    if (initialized) return;
    initialized = true;
    appendLog('[i] 就绪。点击「启动 Antigravity」即可隔离走本地代理（不改系统代理、不开 TUN）。');

    if (tauriListen) {
      state.unlistenAudit = await tauriListen('proxy-audit-progress', (event) => {
        const p = event.payload;
        if (!p) return;
        const pct = Math.min(100, Math.round((p.elapsed / Math.max(1, p.total)) * 100));
        els.auditFill.style.width = `${pct}%`;
        els.auditLabel.textContent = `验收中 ${p.elapsed}/${p.total}s`;
        if (p.snapshot) applySnapshot(p.snapshot);
      });
    }

    if (tauriInvoke) {
      try {
        const s = await tauriInvoke('proxy_get_settings');
        applySettingsToForm(s);
      } catch (err) {
        appendLog(`[!] 加载设置失败: ${err}`);
      }
      await refreshSnapshot();
    }
  }

  return {
    setActive(active) {
      state.active = active;
      if (active) {
        ensureLoaded().then(() => {
          refreshSnapshot();
          startAutoRefresh();
        });
      } else {
        stopAutoRefresh();
      }
    },
  };
}
