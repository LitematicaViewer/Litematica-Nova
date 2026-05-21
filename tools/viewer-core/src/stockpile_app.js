(function () {
  let data = window.__STOCKPILE_DATA__;
  const app = document.getElementById('app');
  const isServeMode = location.protocol === 'http:' || location.protocol === 'https:';
  const isMultiPackage = data && data.manifest && data.manifest.export_mode === 'multi';
  const userKey = 'lba-stockpile-user-id';
  const langKey = 'lba-stockpile-lang';
  let lang = pickInitialLang();
  let userId = localStorage.getItem(userKey) || '';
  let state = {};
  let controls = { search: '', sort: 'grouped', filter: 'all' };
  let open = new Set();
  let collapsedTree = new Set();
  let syncState = { participants: [], materials: {}, updated_at: null };
  let presence = [];
  let lastRevision = null;
  let lastSyncText = '-';
  let syncError = '';
  let pollHandle = null;
  let participantRegistered = false;
  let config = defaultConfig();
  let authStatus = { authenticated: false, admin: false, access_password_enabled: false, whitelist_enabled: false, allow_guest_readonly: false };
  let searchRenderTimer = null;
  let searchComposing = false;
  const activePollMs = 15000;
  const hiddenPollMs = 60000;

  function pickInitialLang() {
    const stored = localStorage.getItem(langKey);
    if (stored === 'zh-CN' || stored === 'en-US') return stored;
    return (navigator.language || '').toLowerCase().startsWith('zh') ? 'zh-CN' : 'en-US';
  }
  function defaultConfig() {
    return {
      mode: 'multi',
      default_language: 'auto',
      poll_interval_ms: 3000,
      show_advanced_recipe_tree: true,
      show_unresolved_recipes: true,
      show_icon_fallback_badge: false
    };
  }
  function applyConfig(nextConfig) {
    config = { ...defaultConfig(), ...(nextConfig || {}) };
    const stored = localStorage.getItem(langKey);
    if (!stored && (config.default_language === 'zh-CN' || config.default_language === 'en-US')) {
      lang = config.default_language;
    }
  }
  const extraI18n = {
    'zh-CN': {
      myTasks: '\u6211\u7684\u4efb\u52a1', myPreparing: '\u6211\u5907\u8d27\u4e2d', myDone: '\u6211\u5df2\u5b8c\u6210', myParticipated: '\u6211\u53c2\u4e0e\u8fc7', myTotal: '\u6211\u7684\u4efb\u52a1\u7edf\u8ba1',
      taskItems: '\u9879\u6570', taskQuantity: '\u6570\u91cf', taskRate: '\u5b8c\u6210\u7387', containerEstimate: '\u5bb9\u5668\u6298\u7b97', rawTotal: '\u603b\u6570\u91cf',
      overfilledOnly: '\u8d85\u989d', stalledOnly: '\u5df2\u5907\u8d27\u4f46\u672a\u5b8c\u6210', lockedOnly: '\u5df2\u9501\u5b9a', notedOnly: '\u6709\u5907\u6ce8', storedOnly: '\u6709\u5b58\u653e\u4f4d\u7f6e',
      copyAllGaps: '\u590d\u5236\u5269\u4f59\u7f3a\u53e3', copyMine: '\u590d\u5236\u6211\u7684\u4efb\u52a1', copyUnclaimed: '\u590d\u5236\u65e0\u4eba\u8ba4\u9886', copyOverfilled: '\u590d\u5236\u8d85\u989d\u6750\u6599',
      copied: '\u5df2\u590d\u5236', readonlyMode: '\u53ea\u8bfb\u6a21\u5f0f', recentBy: '\u6700\u8fd1\u4fee\u6539\u4eba', recentAt: '\u6700\u8fd1\u4fee\u6539\u65f6\u95f4', publicNote: '\u5907\u6ce8', storageLocation: '\u5b58\u653e\u4f4d\u7f6e',
      locked: '\u9501\u5b9a', materialLocked: '\u6750\u6599\u5df2\u9501\u5b9a', loginRequired: '\u8bf7\u5148\u8f93\u5165\u8bbf\u95ee\u5bc6\u7801', readonlyGuest: '\u53ea\u8bfb\u8bbf\u5ba2\u4e0d\u80fd\u4fee\u6539',
      accessPassword: '\u8bbf\u95ee\u5bc6\u7801', noMyTasks: '\u6682\u65e0\u6211\u7684\u4efb\u52a1', rawCountSuffix: '\u539f\u59cb\u4e2a\u6570', operationUnavailable: '\u5f53\u524d\u4e0d\u53ef\u64cd\u4f5c'
      , refresh: '\u5237\u65b0', onlineUsers: '\u5728\u7ebf', onlineCount: '\u5728\u7ebf {n} \u4eba', switchOnly: '\u4ec5\u5207\u6362\u8eab\u4efd', migrateClaims: '\u8fc1\u79fb\u65e7 ID \u4efb\u52a1', mergeIdPrompt: '\u8f93\u5165\u65b0 ID'
    },
    'en-US': {
      myTasks: 'My tasks', myPreparing: 'Preparing', myDone: 'Done', myParticipated: 'Participated', myTotal: 'My task stats',
      taskItems: 'Items', taskQuantity: 'Quantity', taskRate: 'Done rate', containerEstimate: 'Container estimate', rawTotal: 'Total quantity',
      overfilledOnly: 'Overfilled', stalledOnly: 'Prepared not done', lockedOnly: 'Locked', notedOnly: 'With notes', storedOnly: 'With storage',
      copyAllGaps: 'Copy remaining gaps', copyMine: 'Copy my tasks', copyUnclaimed: 'Copy unclaimed', copyOverfilled: 'Copy overfilled',
      copied: 'Copied', readonlyMode: 'Read-only mode', recentBy: 'Last changed by', recentAt: 'Last changed at', publicNote: 'Note', storageLocation: 'Storage',
      locked: 'Locked', materialLocked: 'Material is locked', loginRequired: 'Access password required', readonlyGuest: 'Read-only guest',
      accessPassword: 'Access password', noMyTasks: 'No tasks yet', rawCountSuffix: 'raw', operationUnavailable: 'Unavailable'
      , refresh: 'Refresh', onlineUsers: 'Online', onlineCount: '{n} online', switchOnly: 'Switch only', migrateClaims: 'Migrate old ID tasks', mergeIdPrompt: 'New ID'
    }
  };
  function dict() { return (data.i18n && data.i18n[lang]) || (data.i18n && data.i18n['en-US']) || {}; }
  function t(key) { return extraI18n[lang]?.[key] || dict()[key] || extraI18n['en-US'][key] || defaultLabel(key) || key; }
  function defaultLabel(key) {
    const zh = {
      totalQuantity: '\u603b\u6570\u91cf', rawCount: '\u539f\u59cb\u4e2a\u6570', stackSize: '\u6bcf\u7ec4\u6570\u91cf', unitChest: '\u7bb1\u76d2', unitBox: '\u76d2', unitStack: '\u7ec4', unitEach: '\u4e2a',
      direct: '\u53ef\u91c7\u96c6', recipeDirect: '\u53ef\u76f4\u63a5\u91c7\u96c6\u6216\u6316\u6398\u83b7\u5f97', namespaceId: '\u547d\u540d\u7a7a\u95f4 ID', stacksRemainder: '\u7ec4 / \u4f59\u6570', shulkerBoxes: '\u76d2\u6570',
      tagPlanks: '\u4efb\u610f\u6728\u677f', tagLogs: '\u4efb\u610f\u539f\u6728', tagStone: '\u4efb\u610f\u77f3\u6750', tagCoals: '\u4efb\u610f\u7164\u70ad', tagIronOres: '\u4efb\u610f\u94c1\u77ff\u77f3',
      normalizedFrom: '\u5df2\u5f52\u4e00\u5316\u6765\u6e90', iconDiagnostic: '\u56fe\u6807\u8bca\u65ad'
    };
    const en = {
      totalQuantity: 'Total quantity', rawCount: 'Raw count', stackSize: 'Stack size', unitChest: 'chest-box', unitBox: 'box', unitStack: 'stack', unitEach: 'item',
      direct: 'Gatherable', recipeDirect: 'Directly gather or mine this material', namespaceId: 'Namespace ID', stacksRemainder: 'stacks / remainder', shulkerBoxes: 'boxes',
      tagPlanks: 'Any planks', tagLogs: 'Any logs', tagStone: 'Any stone material', tagCoals: 'Any coal', tagIronOres: 'Any iron ore',
      normalizedFrom: 'Normalized from', iconDiagnostic: 'Icon diagnostic'
    };
    return (lang === 'zh-CN' ? zh : en)[key] || en[key];
  }
  function storageKey() { return `lba-stockpile:${data.manifest.source_file}:${data.manifest.created_at}`; }
  function loadOfflineState() {
    try { state = JSON.parse(localStorage.getItem(storageKey()) || '{}'); }
    catch (_) { state = {}; }
  }
  function saveOfflineState() { localStorage.setItem(storageKey(), JSON.stringify(state)); }

  async function init() {
    if (isServeMode) {
      try {
        const project = await apiGet('/api/project');
        if (project && project.manifest) data = project;
        applyConfig(await apiGet('/api/config'));
        authStatus = await apiGet('/api/auth/status');
      } catch (error) {
        syncError = `${t('syncError')}: ${error.message}`;
      }
    }
    if (!isServeMode && isMultiPackage) {
      syncError = t('multiPreviewWarning') || 'This is a multiplayer package. Run the start script to sync; direct file preview cannot sync.';
    }
    loadOfflineState();
    document.title = t('appTitle');
    if (userId && isServeMode) await registerParticipant();
    await refreshState(true);
    render();
    ensureUser();
    if (isServeMode) {
      const poll = async () => {
        if (userId && !participantRegistered) await registerParticipant();
        await heartbeatPresence();
        await refreshState(false);
        if (!isEditing() && !searchComposing) render();
        pollHandle = setTimeout(poll, document.hidden ? hiddenPollMs : activePollMs);
      };
      pollHandle = setTimeout(poll, activePollMs);
    }
  }

  async function apiGet(path) {
    const response = await fetch(path, { headers: { Accept: 'application/json' } });
    if (!response.ok) throw new Error(`${response.status} ${response.statusText}`);
    return response.json();
  }
  async function apiSend(path, method, body) {
    const response = await fetch(path, {
      method,
      headers: { 'Content-Type': 'application/json', Accept: 'application/json' },
      body: body === undefined ? undefined : JSON.stringify(body)
    });
    if (!response.ok) throw new Error(`${response.status} ${await response.text()}`);
    return response.headers.get('content-type')?.includes('application/json') ? response.json() : null;
  }
  async function registerParticipant() {
    if (!userId || !isServeMode) return;
    try {
      await apiSend('/api/participants', 'POST', { user_id: userId });
      participantRegistered = true;
      syncError = '';
    } catch (error) {
      participantRegistered = false;
      syncError = `${t('syncError')}: ${error.message}`;
    }
  }
  async function refreshState(force) {
    if (!isServeMode) {
      syncState = offlineSyncState();
      return;
    }
    try {
      const revision = await apiGet('/api/revision');
      if (force || revision.revision !== lastRevision) {
        syncState = await apiGet('/api/state');
        lastRevision = revision.revision;
      }
      presence = syncState.presence || presence;
      try { authStatus = await apiGet('/api/auth/status'); } catch (_) {}
      participantRegistered = !!userId && (syncState.participants || []).some((participant) => participant.user_id === userId);
      lastSyncText = new Date().toLocaleTimeString();
      syncError = '';
    } catch (error) {
      syncError = `${t('syncError')}: ${error.message}`;
    }
  }

  async function heartbeatPresence() {
    if (!isServeMode) return;
    try {
      presence = await apiSend('/api/presence', 'POST', { user_id: userId || 'guest' });
    } catch (_) {}
  }

  function offlineSyncState() {
    const materials = {};
    for (const item of data.materials.materials) {
      const local = state[item.namespace_id] || { status: 'not_started', quantity: 0, assignee: '' };
      const quantity = Number(local.quantity || 0);
      const status = local.status === 'done' ? 'done' : local.status === 'in_progress' ? 'preparing' : 'not_started';
      const preparing = status === 'preparing' ? quantity : 0;
      const done = status === 'done' ? quantity : 0;
      const total = preparing + done;
      materials[item.namespace_id] = {
        material_id: item.namespace_id,
        required_count: item.required_count,
        stack_size: item.stack_size || 64,
        preparing_count: preparing,
        done_count: done,
        remaining_count: Math.max(0, item.required_count - total),
        overfilled_count: Math.max(0, total - item.required_count),
        participants: local.assignee ? [local.assignee] : [],
        overall_status: total === 0 ? 'not_started' : total > item.required_count ? 'overfilled' : done >= item.required_count ? 'done' : done > 0 ? 'partial_done' : 'preparing',
        claims: local.assignee ? [{ user_id: local.assignee, status: status === 'done' ? 'done' : 'preparing', quantity, updated_at: Math.floor(Date.now() / 1000) }] : [],
        updated_by: local.assignee || undefined,
        updated_at: local.assignee ? Math.floor(Date.now() / 1000) : undefined
      };
    }
    return withClientSummaries({ participants: userId ? [{ user_id: userId }] : [], materials, updated_at: Date.now() });
  }

  function materialSync(item) {
    return syncState.materials[item.namespace_id] || {
      material_id: item.namespace_id,
      required_count: item.required_count,
      stack_size: item.stack_size || 64,
      preparing_count: 0,
      done_count: 0,
      remaining_count: item.required_count,
      overfilled_count: 0,
      participants: [],
      overall_status: 'not_started',
      claims: [],
      public_note: undefined,
      storage_location: undefined,
      locked: false,
      updated_by: undefined,
      updated_at: undefined
    };
  }
  function myClaim(item) {
    const sync = materialSync(item);
    return (sync.claims || []).find((claim) => claim.user_id === userId) || { status: 'preparing', quantity: 0 };
  }

  function ensureUser() {
    if (userId.trim()) return true;
    renderModal();
    return false;
  }
  function canWrite(item) {
    const sync = item ? materialSync(item) : {};
    if (!isServeMode) return true;
    if (sync.locked && !authStatus.admin) return false;
    if (authStatus.access_password_enabled && !authStatus.authenticated) return false;
    if (authStatus.whitelist_enabled && authStatus.allow_guest_readonly && !authStatus.admin) return false;
    return true;
  }
  function writeHint(item) {
    const sync = item ? materialSync(item) : {};
    if (sync.locked && !authStatus.admin) return t('materialLocked') || 'Locked';
    if (authStatus.access_password_enabled && !authStatus.authenticated) return t('loginRequired') || 'Login required';
    if (authStatus.whitelist_enabled && authStatus.allow_guest_readonly && !authStatus.admin) return t('readonlyGuest') || 'Read-only guest';
    return '';
  }
  function renderAccessLogin() {
    if (!isServeMode || !authStatus.access_password_enabled || authStatus.authenticated) return '';
    return `<section class="toolbar"><div class="password-row"><input class="field" id="accessPassword" type="password" placeholder="${escapeAttr(t('accessPassword') || 'Access password')}" /><button class="button icon-button" id="toggleAccessPassword" type="button" title="${escapeAttr(t('accessPassword') || 'Access password')}">\u663e\u793a</button></div><button class="button primary" id="accessLogin">${escapeHtml(t('enter') || 'Enter')}</button></section>`;
  }
  function renderModal() {
    if (document.querySelector('.modal')) return;
    const modal = document.createElement('div');
    modal.className = 'modal';
    modal.innerHTML = `<div class="modal-card">
      <h2>${escapeHtml(t('enterIdTitle'))}</h2>
      <p>${escapeHtml(t('enterIdBody'))}</p>
      <input class="field" id="userInput" autocomplete="off" placeholder="${escapeAttr(t('enterIdPlaceholder'))}" />
      <div class="actions"><button class="button primary" id="saveUser">${escapeHtml(t('enter'))}</button></div>
    </div>`;
    document.body.appendChild(modal);
    const input = modal.querySelector('#userInput');
    input.focus();
    modal.querySelector('#saveUser').addEventListener('click', async () => {
      const value = input.value.trim();
      if (!value) { input.focus(); return; }
      userId = value;
      participantRegistered = false;
      localStorage.setItem(userKey, userId);
      modal.remove();
      if (isServeMode) await registerParticipant();
      await refreshState(true);
      render();
    });
    input.addEventListener('keydown', (event) => {
      if (event.key === 'Enter') modal.querySelector('#saveUser').click();
    });
  }

  function filteredMaterials() {
    const query = controls.search.trim().toLowerCase();
    return data.materials.materials.filter((item) => {
      const sync = materialSync(item);
      const mine = (sync.claims || []).some((claim) => claim.user_id === userId);
      if (query && !`${displayName(item)} ${item.display_name} ${item.namespace_id} ${item.category} ${item.item_icon_key}`.toLowerCase().includes(query)) return false;
      switch (controls.filter) {
        case 'not_started': return sync.overall_status === 'not_started';
        case 'preparing': return sync.overall_status === 'preparing' || sync.overall_status === 'partial_done';
        case 'done': return sync.overall_status === 'done';
        case 'mine': return mine;
        case 'unclaimed': return !sync.participants.length;
        case 'overfilled': return sync.overfilled_count > 0;
        case 'stalled': return sync.preparing_count > 0 && sync.done_count < sync.required_count;
        case 'locked': return !!sync.locked;
        case 'noted': return !!sync.public_note;
        case 'stored': return !!sync.storage_location;
        case 'available': return item.recipe_status === 'available';
        case 'unresolved': return item.recipe_status === 'unresolved';
        case 'missing': return item.recipe_status === 'missing';
        default: return true;
      }
    }).sort(compareMaterials);
  }
  function compareMaterials(a, b) {
    const sa = materialSync(a);
    const sb = materialSync(b);
    const mineA = (sa.claims || []).some((claim) => claim.user_id === userId) ? 0 : 1;
    const mineB = (sb.claims || []).some((claim) => claim.user_id === userId) ? 0 : 1;
    switch (controls.sort) {
      case 'count_asc': return a.required_count - b.required_count || displayName(a).localeCompare(displayName(b));
      case 'name': return displayName(a).localeCompare(displayName(b));
      case 'remaining_desc': return sb.remaining_count - sa.remaining_count || b.required_count - a.required_count;
      case 'status': return statusRank(sa.overall_status) - statusRank(sb.overall_status) || b.required_count - a.required_count;
      case 'mine': return mineA - mineB || b.required_count - a.required_count;
      case 'grouped': return a.category.localeCompare(b.category) || statusRank(sa.overall_status) - statusRank(sb.overall_status) || b.required_count - a.required_count;
      case 'count_desc':
      default: return b.required_count - a.required_count || displayName(a).localeCompare(displayName(b));
    }
  }
  function statusRank(status) {
    return { not_started: 0, preparing: 1, partial_done: 2, done: 3, overfilled: 4 }[status] ?? 5;
  }
  function grouped(items) {
    const groups = new Map();
    for (const item of items) {
      if (!groups.has(item.category)) groups.set(item.category, []);
      groups.get(item.category).push(item);
    }
    return [...groups.entries()].map(([category, group]) => ({ category, items: group.sort((a, b) => {
      const sa = materialSync(a);
      const sb = materialSync(b);
      return statusRank(sa.overall_status) - statusRank(sb.overall_status) || b.required_count - a.required_count;
    }) }));
  }
  function totals(items) {
    const required = items.reduce((sum, item) => sum + item.required_count, 0);
    const done = items.reduce((sum, item) => sum + materialSync(item).done_count, 0);
    const assigned = items.reduce((sum, item) => sum + materialSync(item).preparing_count + materialSync(item).done_count, 0);
    return { required, done, assigned, progress: required ? Math.min(100, Math.round(done / required * 100)) : 0 };
  }
  function stateSummaries() {
    return syncState.summaries || withClientSummaries(syncState).summaries || {};
  }
  function myTaskStats() {
    const my = (stateSummaries().user_summaries || []).find((item) => item.user_id === userId);
    return my || { material_count: 0, preparing_count: 0, done_count: 0, preparing_quantity: 0, done_quantity: 0 };
  }
  function withClientSummaries(source) {
    const materials = source.materials || {};
    const users = new Map();
    const summaries = {
      user_summaries: [],
      unclaimed_materials: [],
      overfilled_materials: [],
      stalled_materials: [],
      not_started_materials: [],
      locked_materials: [],
      noted_materials: [],
      recent_activity: source.summaries?.recent_activity || []
    };
    for (const [id, sync] of Object.entries(materials)) {
      if (!(sync.participants || []).length) summaries.unclaimed_materials.push(id);
      if (sync.overfilled_count > 0) summaries.overfilled_materials.push(id);
      if (sync.preparing_count > 0 && sync.done_count < sync.required_count) summaries.stalled_materials.push(id);
      if (sync.overall_status === 'not_started') summaries.not_started_materials.push(id);
      if (sync.locked) summaries.locked_materials.push(id);
      if (sync.public_note) summaries.noted_materials.push(id);
      for (const claim of sync.claims || []) {
        const user = users.get(claim.user_id) || { user_id: claim.user_id, material_count: 0, preparing_count: 0, done_count: 0, preparing_quantity: 0, done_quantity: 0 };
        user.material_count += 1;
        if (claim.status === 'done') { user.done_count += 1; user.done_quantity += Number(claim.quantity || 0); }
        else { user.preparing_count += 1; user.preparing_quantity += Number(claim.quantity || 0); }
        users.set(claim.user_id, user);
      }
    }
    summaries.user_summaries = [...users.values()];
    return { ...source, summaries };
  }

  function render() {
    document.title = t('appTitle');
    const active = document.activeElement;
    const restoreSearch = active?.id === 'search';
    const searchStart = restoreSearch ? active.selectionStart : null;
    const searchEnd = restoreSearch ? active.selectionEnd : null;
    const items = filteredMaterials();
    const allTotals = totals(data.materials.materials);
    const mine = myTaskStats();
    app.innerHTML = `<header class="topbar">
      <div class="brand"><strong>${escapeHtml(t('appTitle'))}</strong><span>${escapeHtml(data.manifest.source_file)}</span></div>
      <div class="identity">
        <span class="note">${escapeHtml(isServeMode ? t('syncMode') : t('offlineMode'))}</span>
        ${writeHint() ? `<span class="note">${escapeHtml(t('readonlyMode'))}: ${escapeHtml(writeHint())}</span>` : ''}
        <span class="badge">${escapeHtml(t('currentId'))}: ${escapeHtml(userId || '-')}</span>
        ${isServeMode ? `<span class="badge">${escapeHtml(t('lastSync'))}: ${escapeHtml(lastSyncText)}</span>${config.mode === 'multi' ? `<span class="badge">${escapeHtml(t('participants'))}: ${(syncState.participants || []).length}</span>${renderPresenceBadge()}` : ''}` : ''}
        <select class="field" id="lang" aria-label="${escapeAttr(t('language'))}"><option value="zh-CN" ${lang === 'zh-CN' ? 'selected' : ''}>\u4e2d\u6587</option><option value="en-US" ${lang === 'en-US' ? 'selected' : ''}>English</option></select>
        <button class="button" id="refreshNow">${escapeHtml(t('refresh') || 'Refresh')}</button>
        <button class="button" id="switchUser">${escapeHtml(t('switchId'))}</button>
      </div>
    </header>
    <main class="shell">
      ${syncError ? `<div class="empty">${escapeHtml(syncError)}</div>` : ''}
      <section class="summary">
        ${metric(t('totalMaterials'), data.materials.summary.unique_materials)}
        ${metric(t('rawTotal'), rawQuantity(data.materials.summary.total_blocks))}
        ${metric(t('containerEstimate'), formatQuantity(data.materials.summary.total_blocks))}
        <div class="metric"><b>${allTotals.progress}%</b><span>${escapeHtml(t('done'))}</span><div class="progress-track"><div class="progress-bar" style="width:${allTotals.progress}%"></div></div></div>
      </section>
      ${renderMyTasks(mine)}
      <section class="toolbar">
        <input class="field" id="search" value="${escapeAttr(controls.search)}" placeholder="${escapeAttr(t('search'))}" />
        <select class="field" id="sort">${sortOptions()}</select>
        <select class="field" id="filter">${filterOptions()}</select>
      </section>
      <section class="toolbar copy-toolbar">
        <button class="button" data-copy="all">${escapeHtml(t('copyAllGaps'))}</button>
        <button class="button" data-copy="mine">${escapeHtml(t('copyMine'))}</button>
        <button class="button" data-copy="unclaimed">${escapeHtml(t('copyUnclaimed'))}</button>
        <button class="button" data-copy="overfilled">${escapeHtml(t('copyOverfilled'))}</button>
      </section>
      ${renderAccessLogin()}
      ${renderList(items)}
    </main>`;
    bindControls();
    if (restoreSearch) {
      const nextSearch = document.getElementById('search');
      if (nextSearch) {
        nextSearch.focus();
        try { nextSearch.setSelectionRange(searchStart, searchEnd); } catch (_) {}
      }
    }
  }
  function metric(label, value) {
    return `<div class="metric"><b>${escapeHtml(String(value))}</b><span>${escapeHtml(label)}</span></div>`;
  }
  function renderPresenceBadge() {
    const users = (presence || syncState.presence || []).map((entry) => entry.user_id).filter(Boolean);
    if (!users.length) return '';
    const label = users.length <= 4 ? `${t('onlineUsers')}: ${users.join(', ')}` : (t('onlineCount') || '{n} online').replace('{n}', users.length);
    return `<details class="presence"><summary class="badge">${escapeHtml(label)}</summary><div class="presence-menu">${users.map((id) => `<span>${escapeHtml(id)}</span>`).join('')}</div></details>`;
  }
  function renderMyTasks(mine) {
    const tasks = myTaskItems();
    const doneRate = mine.material_count ? Math.round(mine.done_count / mine.material_count * 100) : 0;
    const preparing = tasks.filter((task) => task.claim.status !== 'done');
    const done = tasks.filter((task) => task.claim.status === 'done');
    return `<section class="section my-tasks">
      <div class="section-head"><div><h2>${escapeHtml(t('myTasks'))}</h2><div class="section-meta">${escapeHtml(t('taskItems'))}: ${mine.material_count} / ${escapeHtml(t('taskQuantity'))}: ${escapeHtml(formatQuantity(mine.preparing_quantity + mine.done_quantity))} / ${escapeHtml(t('taskRate'))}: ${doneRate}%</div></div><div class="section-rate">${doneRate}%</div></div>
      <div class="task-columns">
        ${taskColumn(t('myPreparing'), preparing)}
        ${taskColumn(t('myDone'), done)}
        ${taskColumn(t('myParticipated'), tasks)}
      </div>
    </section>`;
  }
  function taskColumn(title, tasks) {
    const body = tasks.slice(0, 8).map(({ item, claim }) => `<li>${escapeHtml(displayName(item))}<span>${escapeHtml(formatQuantity(claim.quantity, item))}</span></li>`).join('');
    return `<div class="task-panel"><h3>${escapeHtml(title)}</h3><ul>${body || `<li>${escapeHtml(t('noMyTasks'))}</li>`}</ul></div>`;
  }
  function myTaskItems() {
    return data.materials.materials.flatMap((item) => (materialSync(item).claims || [])
      .filter((claim) => claim.user_id === userId)
      .map((claim) => ({ item, claim })));
  }
  function sortOptions() {
    return [
      ['count_desc', t('countDesc')], ['count_asc', t('countAsc')], ['grouped', t('grouped')],
      ['remaining_desc', t('remainingDesc')], ['status', t('status')], ['mine', t('mineFirst')], ['name', t('name')]
    ].map(([value, label]) => `<option value="${value}" ${controls.sort === value ? 'selected' : ''}>${escapeHtml(label)}</option>`).join('');
  }
  function filterOptions() {
    return [
      ['all', t('all')], ['not_started', t('notStarted')], ['preparing', t('preparing')], ['done', t('done')],
      ['mine', t('mine')], ['unclaimed', t('unclaimed')], ['overfilled', t('overfilledOnly')], ['stalled', t('stalledOnly')],
      ['locked', t('lockedOnly')], ['noted', t('notedOnly')], ['stored', t('storedOnly')], ['available', t('craftable')], ['unresolved', t('unresolved')], ['missing', t('recipeMissing')]
    ].map(([value, label]) => `<option value="${value}" ${controls.filter === value ? 'selected' : ''}>${escapeHtml(label)}</option>`).join('');
  }
  function renderList(items) {
    if (!items.length) return `<div class="empty">${escapeHtml(t('empty'))}</div>`;
    if (controls.sort !== 'grouped') return `<section class="section"><div class="list">${items.map(renderCard).join('')}</div></section>`;
    return grouped(items).map(renderGroup).join('');
  }
  function renderGroup(group) {
    const info = totals(group.items);
    const icon = group.items[0]?.category_icon || 'minecraft:barrier';
    return `<section class="section">
      <div class="section-head"><div class="section-icon">${iconImg(icon, true)}</div><div><h2>${escapeHtml(group.category)}</h2><div class="section-meta">${group.items.length} / ${escapeHtml(t('required'))} ${escapeHtml(formatQuantity(info.required))} / ${escapeHtml(t('claimed'))} ${escapeHtml(formatQuantity(info.assigned))}</div></div><div class="section-rate">${info.progress}%</div></div>
      <div class="list">${group.items.map(renderCard).join('')}</div>
    </section>`;
  }
  function renderCard(item) {
    const sync = materialSync(item);
    const claim = myClaim(item);
    const isOpen = open.has(item.namespace_id);
    const disabled = !canWrite(item);
    const hint = writeHint(item);
    const actionControls = disabled
      ? `<span class="sub">${escapeHtml(hint || t('operationUnavailable'))}</span>`
      : `<input class="field qty" type="number" min="0" value="${Number(claim.quantity || 0)}" data-action="qty" />
          <button class="button" data-action="progress">${escapeHtml(t('preparing'))}</button>
          <button class="button primary" data-action="done">${escapeHtml(t('done'))}</button>
          <button class="button danger" data-action="cancel">${escapeHtml(t('cancel'))}</button>`;
    const updatedText = [
      sync.public_note ? `${t('publicNote')}: ${sync.public_note}` : '',
      sync.storage_location ? `${t('storageLocation')}: ${sync.storage_location}` : '',
      sync.updated_by ? `${t('recentBy')}: ${sync.updated_by}` : '',
      sync.updated_at ? `${t('recentAt')}: ${formatTime(sync.updated_at)}` : '',
      hint
    ].filter(Boolean).join(' / ');
    return `<article class="card ${isOpen ? 'open' : ''}" data-id="${escapeAttr(item.namespace_id)}">
      <div class="card-main">
        <div>
          <div class="material-title">${iconImg(item.item_icon_key, true, item)}<span class="name">${escapeHtml(displayName(item))}</span></div>
          <div class="sub">${escapeHtml(item.namespace_id)} / ${escapeHtml(item.category)} / ${escapeHtml(t('remaining'))} ${escapeHtml(formatQuantity(sync.remaining_count, item))}</div>
          <div class="badges"><span class="badge ${item.recipe_status}">${recipeLabel(item.recipe_status)}</span><span class="badge ${sync.overall_status}">${overallLabel(sync.overall_status)}</span><span class="badge">${sync.participants.length ? `${escapeHtml(t('claimedBy'))}: ${escapeHtml(sync.participants.join(', '))}` : escapeHtml(t('unclaimed'))}</span>${sync.locked ? `<span class="badge missing">${escapeHtml(t('locked') || 'Locked')}</span>` : ''}${config.show_icon_fallback_badge && item.icon_available === false ? `<span class="badge missing">${escapeHtml(t('iconFallback'))}</span>` : ''}</div>
          ${updatedText ? `<div class="sub">${escapeHtml(updatedText)}</div>` : ''}
        </div>
        <div><div class="count">${formatQuantity(item.required_count, item)}</div><div class="actions">
          ${actionControls}
          <button class="button" data-action="toggle">${escapeHtml(isOpen ? t('collapse') : t('details'))}</button>
        </div></div>
      </div>
      <div class="details">${renderDetails(item)}</div>
    </article>`;
  }
  function renderDetails(item) {
    const tree = (data.recipe_trees || {})[item.namespace_id];
    const sync = materialSync(item);
    let recipeNote = '';
    if (data.recipe_status.status === 'missing') recipeNote = t('recipeUnavailable');
    else if (item.recipe_status === 'available' && !tree) recipeNote = t('recipeCachedNoTree');
    else if (item.recipe_status === 'unresolved') recipeNote = t('recipeUnresolved');
    return `<div class="detail-grid">
      ${detail(t('namespaceId'), item.namespace_id)}
      ${detail(t('quantity'), formatQuantity(item.required_count, item))}
      ${detail(t('rawCount'), item.required_count)}
      ${detail(t('stackSize'), effectiveStackSize(item))}
      ${detail(t('stacksRemainder'), `${quantityBreakdown(item.required_count, item).stacks} / ${quantityBreakdown(item.required_count, item).remainder}`)}
      ${detail(t('shulkerBoxes'), quantityBreakdown(item.required_count, item).boxes)}
      ${detail(t('sourceRegions'), (item.source_regions || []).join(', ') || '-')}
      ${detail(t('recipeStatus'), recipeLabel(item.recipe_status))}
      ${(item.normalized_from || []).length ? detail(t('normalizedFrom') || 'Normalized from', item.normalized_from.join(', ')) : ''}
      ${item.icon_diagnostic ? detail(t('iconDiagnostic') || 'Icon diagnostic', item.icon_diagnostic) : ''}
    </div>
    <div class="claim-list">${(sync.claims || []).map((claim) => `<span class="claim-chip">${escapeHtml(claim.user_id)} / ${escapeHtml(overallLabel(claim.status))} / ${escapeHtml(formatQuantity(claim.quantity, item))} (${escapeHtml(rawQuantity(claim.quantity))})</span>`).join('')}</div>
    ${tree && config.show_advanced_recipe_tree ? `<div class="craft-chain">${renderRecipeTree(tree, true)}</div>` : ''}${recipeNote && config.show_unresolved_recipes ? `<p class="sub">${escapeHtml(recipeNote)}</p>` : ''}`;
  }
  function renderRecipeTree(node, isRoot = true) {
    const nodeId = node.node_id || `${node.item_id}-${node.depth || 0}`;
    const collapsed = collapsedTree.has(nodeId);
    const hasChildren = (node.children || []).length > 0;
    const children = hasChildren && !collapsed ? (node.children || []).map((child) => renderRecipeTree(child, false)).join('') : '';
    const ingredients = (node.ingredients || []).map((ingredient) => `${itemName(ingredient.item_id, ingredient.display_name || ingredient.item_id)} ${formatQuantity(ingredient.needed_count, materialById(ingredient.item_id))}${ingredient.unresolved ? ` (${reasonLabel(ingredient.unresolved_reason)})` : ''}`).join(' / ');
    const possible = renderPossibleItems(node);
    const message = nodeMessage(node);
    const iconKey = node.visual_kind === 'tag' ? (node.icon_key || '__tag') : node.visual_kind === 'special' ? '__special' : node.unresolved ? '__unresolved' : node.icon_key;
    return `<div class="tree-node ${isRoot ? 'root' : ''}" data-tree-id="${escapeAttr(nodeId)}">
      <div class="recipe-card ${escapeAttr(node.visual_kind || '')}">
        <div class="recipe-card-head">
          ${iconImg(iconKey, true)}
          <div>
            <div class="recipe-name">${escapeHtml(itemName(node.item_id, node.display_name || node.item_id))}</div>
            <div class="recipe-id">${escapeHtml(node.visual_kind === 'tag' ? itemName(node.item_id, node.display_name || node.item_id) : node.item_id)}</div>
            <div class="recipe-badges">
              <span class="recipe-badge process">${processLabel(node)}</span>
              <span class="recipe-badge">${escapeHtml(t('need'))} ${escapeHtml(formatQuantity(node.needed_count, materialById(node.item_id)))}</span>
              ${node.unresolved ? `<span class="recipe-badge bad">${escapeHtml(reasonLabel(node.unresolved_reason))}</span>` : ''}
              ${node.requires_fuel ? `<span class="recipe-badge warn">${escapeHtml(t('fuelRequired'))}</span>` : ''}
              ${node.decorative_smithing ? `<span class="recipe-badge warn">${escapeHtml(t('decorativeSmithing'))}</span>` : ''}
            </div>
          </div>
          ${hasChildren ? `<button class="button tree-toggle" data-action="tree-toggle" data-tree-id="${escapeAttr(nodeId)}">${collapsed ? '+' : '-'}</button>` : ''}
        </div>
        <div class="recipe-grid">
          ${recipeStat(t('recipe'), recipeTypeLabel(node.recipe_type))}
          ${recipeStat(t('process'), processLabel(node))}
          ${recipeStat(t('outputEach'), node.output_count)}
          ${recipeStat(t('batches'), node.batch_count)}
          ${recipeStat(t('extra'), node.extra_output)}
          ${recipeStat(t('depth'), node.depth || 0)}
        </div>
        ${ingredients ? `<div class="recipe-ingredients">${escapeHtml(t('inputs'))}: ${escapeHtml(ingredients)}</div>` : ''}
        ${message ? `<div class="recipe-ingredients">${escapeHtml(message)}</div>` : ''}
        ${possible}
      </div>
      ${children ? `<div class="recipe-children">${children}</div>` : ''}
    </div>`;
  }
  function recipeStat(label, value) {
    return `<div class="recipe-stat"><span>${escapeHtml(label)}</span>${escapeHtml(String(value ?? 0))}</div>`;
  }
  function processLabel(node) {
    const map = {
      craft: 'processCraft',
      stonecut: 'processStonecut',
      smelt: 'processSmelt',
      blast: 'processBlast',
      smoke: 'processSmoke',
      campfire: 'processCampfire',
      smith: 'processSmith',
      smith_trim: 'processSmith',
      special: 'processSpecial',
      tag: 'processTag',
      direct: 'direct',
      unresolved: 'processUnresolved'
    };
    return escapeHtml(t(map[node.process_type] || 'processUnresolved'));
  }
  function recipeTypeLabel(recipeType) {
    const map = {
      'minecraft:crafting_shaped': 'recipeCraftingShaped',
      'minecraft:crafting_shapeless': 'recipeCraftingShapeless',
      'minecraft:stonecutting': 'recipeStonecutting',
      'minecraft:smelting': 'recipeSmelting',
      'minecraft:blasting': 'recipeBlasting',
      'minecraft:smoking': 'recipeSmoking',
      'minecraft:campfire_cooking': 'recipeCampfireCooking',
      'minecraft:smithing_transform': 'recipeSmithingTransform',
      'minecraft:smithing_trim': 'recipeSmithingTrim',
      direct: 'direct'
    };
    if ((recipeType || '').startsWith('minecraft:crafting_special_')) return t('recipeSpecial');
    return t(map[recipeType] || 'processUnresolved');
  }
  function renderPossibleItems(node) {
    const values = (node.possible_items || []).slice(0, 12);
    if (!values.length) return '';
    const more = (node.possible_items || []).length - values.length;
    return `<div class="possible-items">${values.map((item) => `<span>${escapeHtml(itemName(item, item))}</span>`).join('')}${more > 0 ? `<span>+${more}</span>` : ''}</div>`;
  }
  function nodeMessage(node) {
    if (node.visual_kind === 'tag') return `${t('tagGroup')} ${itemName(node.item_id, node.display_name || node.item_id)}`;
    if (node.visual_kind === 'direct') return t('recipeDirect');
    if (node.visual_kind === 'special') return `${t('specialRecipe')}: ${node.recipe_type}`;
    if (node.unresolved_reason === 'no_recipe') return t('noRecipe');
    if (node.unresolved_reason === 'tag_input') return t('tagInput');
    if (node.unresolved) return `${t('unresolved')}: ${reasonLabel(node.unresolved_reason)}`;
    return '';
  }
  function detail(label, value) { return `<div class="detail"><span>${escapeHtml(label)}</span>${escapeHtml(String(value))}</div>`; }
  function recipeLabel(status) {
    return { available: t('available'), direct: t('direct'), unresolved: t('unresolved'), missing: t('missing') }[status] || status;
  }
  function overallLabel(status) {
    return { not_started: t('notStarted'), preparing: t('preparing'), partial_done: t('partialDone'), done: t('done'), overfilled: t('overfilled') }[status] || status;
  }
  function reasonLabel(reason) {
    return { tag_input: t('tagInput'), special_recipe: t('specialRecipe'), no_recipe: t('noRecipe'), depth_limit: 'depth_limit', cycle: 'cycle' }[reason] || reason || t('unresolved');
  }
  function iconImg(key, large, material) {
    const src = material?.icon_path || (data.icons?.by_key?.[key]?.path) || data.icons?.by_key?.__fallback?.path || '';
    const fallback = material && material.icon_available === false ? ' fallback' : '';
    return `<span class="icon-frame ${large ? 'large' : ''}${fallback}"><img class="icon-img" src="${escapeAttr(src)}" alt="" loading="lazy" /></span>`;
  }
  function displayName(item) {
    return item.display_names?.[lang] || item.display_names?.['en-US'] || item.display_name || item.namespace_id;
  }
  function itemName(itemId, fallback) {
    if ((itemId || '').startsWith('#')) return tagName(itemId);
    return data.item_names?.names?.[itemId]?.[lang] || data.item_names?.names?.[itemId]?.['en-US'] || fallback || itemId;
  }
  function tagName(tag) {
    return {
      '#minecraft:planks': t('tagPlanks'),
      '#minecraft:logs': t('tagLogs'),
      '#minecraft:logs_that_burn': t('tagLogs'),
      '#minecraft:stone_crafting_materials': t('tagStone'),
      '#minecraft:stone_tool_materials': t('tagStone'),
      '#minecraft:coals': t('tagCoals'),
      '#minecraft:iron_ores': t('tagIronOres')
    }[tag] || t('tagGroup');
  }
  function effectiveStackSize(item) {
    return Number(materialSync(item).stack_size || item.stack_size || 64);
  }
  function materialById(itemId) {
    return data.materials.materials.find((item) => item.namespace_id === itemId);
  }
  function rawQuantity(count) {
    return `${Math.max(0, Number(count || 0))}${t('unitEach')}`;
  }
  function formatQuantity(count, item) {
    let remaining = Math.max(0, Number(count || 0));
    const stackSize = item ? effectiveStackSize(item) : 64;
    const boxSize = stackSize * 27;
    const chestSize = boxSize * 27;
    const parts = [];
    if (remaining >= chestSize) {
      const chests = Math.floor(remaining / chestSize);
      parts.push(`${chests}${t('unitChest')}`);
      remaining %= chestSize;
    }
    if (remaining >= boxSize) {
      const boxes = Math.floor(remaining / boxSize);
      parts.push(`${boxes}${t('unitBox')}`);
      remaining %= boxSize;
    }
    if (remaining >= stackSize) {
      const stacks = Math.floor(remaining / stackSize);
      parts.push(`${stacks}${t('unitStack')}`);
      remaining %= stackSize;
    }
    if (remaining > 0 || !parts.length) parts.push(`${remaining}${t('unitEach')}`);
    return parts.join(' ');
  }
  function quantityBreakdown(count, item) {
    const stackSize = item ? effectiveStackSize(item) : 64;
    const total = Math.max(0, Number(count || 0));
    const stacks = Math.floor(total / stackSize);
    const remainder = total % stackSize;
    return { stacks, remainder, boxes: Math.ceil((stacks + (remainder > 0 ? 1 : 0)) / 27) };
  }
  function formatTime(seconds) {
    const value = Number(seconds || 0);
    if (!value) return '-';
    return new Date(value * 1000).toLocaleString();
  }

  function bindControls() {
    document.getElementById('refreshNow')?.addEventListener('click', async () => {
      await refreshState(true);
      render();
    });
    document.getElementById('switchUser').addEventListener('click', async () => {
      const oldId = userId;
      const nextId = prompt(t('mergeIdPrompt'), oldId || '');
      if (nextId === null) return;
      const trimmed = nextId.trim();
      if (!trimmed) {
        localStorage.removeItem(userKey);
        userId = '';
        participantRegistered = false;
        ensureUser();
        render();
        return;
      }
      const migrate = oldId && oldId !== trimmed && confirm(`${t('migrateClaims')}?`);
      userId = trimmed;
      localStorage.setItem(userKey, userId);
      participantRegistered = false;
      if (isServeMode && oldId && oldId !== userId) {
        await apiSend('/api/users/merge', 'POST', { from_user_id: oldId, to_user_id: userId, mode: migrate ? 'migrate' : 'switch' });
      }
      if (isServeMode) await registerParticipant();
      await refreshState(true);
      render();
    });
    document.getElementById('lang').addEventListener('change', (event) => {
      lang = event.target.value;
      localStorage.setItem(langKey, lang);
      document.title = t('appTitle');
      render();
    });
    const search = document.getElementById('search');
    search.addEventListener('compositionstart', () => { searchComposing = true; });
    search.addEventListener('compositionend', (event) => {
      searchComposing = false;
      controls.search = event.target.value;
      render();
    });
    search.addEventListener('input', (event) => {
      controls.search = event.target.value;
      if (searchComposing) return;
      clearTimeout(searchRenderTimer);
      searchRenderTimer = setTimeout(render, 180);
    });
    document.getElementById('sort').addEventListener('change', (event) => { controls.sort = event.target.value; render(); });
    document.getElementById('filter').addEventListener('change', (event) => { controls.filter = event.target.value; render(); });
    const accessLogin = document.getElementById('accessLogin');
    if (accessLogin) accessLogin.addEventListener('click', async () => {
      try {
        await apiSend('/api/auth/access', 'POST', { password: document.getElementById('accessPassword').value, user_id: userId || null });
        authStatus = await apiGet('/api/auth/status');
        await refreshState(true);
        render();
      } catch (error) { syncError = `${t('syncError')}: ${error.message}`; render(); }
    });
    const toggleAccessPassword = document.getElementById('toggleAccessPassword');
    if (toggleAccessPassword) toggleAccessPassword.addEventListener('click', () => {
      const input = document.getElementById('accessPassword');
      input.type = input.type === 'password' ? 'text' : 'password';
    });
    document.querySelectorAll('[data-copy]').forEach((button) => {
      button.addEventListener('click', async () => {
        await copyText(buildCopyText(button.dataset.copy));
      });
    });
    document.querySelectorAll('.card').forEach((card) => {
      const item = data.materials.materials.find((material) => material.namespace_id === card.dataset.id);
      card.querySelector('[data-action="progress"]')?.addEventListener('click', () => updateClaim(item, 'preparing'));
      card.querySelector('[data-action="done"]')?.addEventListener('click', () => updateClaim(item, 'done'));
      card.querySelector('[data-action="cancel"]')?.addEventListener('click', () => deleteClaim(item));
      card.querySelector('[data-action="toggle"]')?.addEventListener('click', () => { open.has(item.namespace_id) ? open.delete(item.namespace_id) : open.add(item.namespace_id); render(); });
    });
    document.querySelectorAll('[data-action="tree-toggle"]').forEach((button) => {
      button.addEventListener('click', (event) => {
        event.stopPropagation();
        const id = button.dataset.treeId;
        collapsedTree.has(id) ? collapsedTree.delete(id) : collapsedTree.add(id);
        render();
      });
    });
  }
  async function updateClaim(item, status) {
    if (!ensureUser()) return;
    const card = document.querySelector(`.card[data-id="${cssEscape(item.namespace_id)}"]`);
    const entered = Math.max(0, Number(card?.querySelector('[data-action="qty"]')?.value || 0));
    const quantity = entered > 0 ? entered : item.required_count;
    if (isServeMode) {
      await apiSend(`/api/materials/${encodeURIComponent(item.namespace_id)}/claims/${encodeURIComponent(userId)}`, 'PUT', { status, quantity });
      await refreshState(true);
    } else {
      state[item.namespace_id] = { status: status === 'done' ? 'done' : 'in_progress', quantity, assignee: userId };
      saveOfflineState();
      syncState = offlineSyncState();
    }
    render();
  }
  async function deleteClaim(item) {
    if (!ensureUser()) return;
    if (isServeMode) {
      await apiSend(`/api/materials/${encodeURIComponent(item.namespace_id)}/claims/${encodeURIComponent(userId)}`, 'DELETE');
      await refreshState(true);
    } else {
      delete state[item.namespace_id];
      saveOfflineState();
      syncState = offlineSyncState();
    }
    render();
  }
  function buildCopyText(kind) {
    let items = data.materials.materials;
    if (kind === 'mine') items = items.filter((item) => (materialSync(item).claims || []).some((claim) => claim.user_id === userId));
    if (kind === 'unclaimed') items = items.filter((item) => !(materialSync(item).participants || []).length);
    if (kind === 'overfilled') items = items.filter((item) => materialSync(item).overfilled_count > 0);
    if (kind === 'all') items = items.filter((item) => materialSync(item).remaining_count > 0);
    const title = {
      all: t('copyAllGaps'),
      mine: t('copyMine'),
      unclaimed: t('copyUnclaimed'),
      overfilled: t('copyOverfilled')
    }[kind] || t('appTitle');
    const lines = [`# ${title}`, `${t('currentId')}: ${userId || '-'}`, ''];
    for (const item of items) {
      const sync = materialSync(item);
      const count = kind === 'overfilled' ? sync.overfilled_count : kind === 'mine' ? (sync.claims || []).filter((claim) => claim.user_id === userId).reduce((sum, claim) => sum + Number(claim.quantity || 0), 0) : sync.remaining_count;
      if (count <= 0) continue;
      lines.push(`- ${displayName(item)} (${item.namespace_id}) ${formatQuantity(count, item)} (${rawQuantity(count)}) [${overallLabel(sync.overall_status)}]`);
    }
    return lines.join('\n');
  }
  async function copyText(text) {
    try {
      if (navigator.clipboard?.writeText) {
        try { await navigator.clipboard.writeText(text); }
        catch (_) { fallbackCopyText(text); }
      } else fallbackCopyText(text);
      syncError = t('copied');
    } catch (error) {
      syncError = `${t('syncError')}: ${error.message}`;
    }
    render();
  }
  function fallbackCopyText(text) {
    const area = document.createElement('textarea');
    area.value = text;
    area.setAttribute('readonly', '');
    area.style.position = 'fixed';
    area.style.left = '-9999px';
    document.body.appendChild(area);
    area.select();
    document.execCommand('copy');
    area.remove();
  }
  function cssEscape(value) {
    return String(value).replace(/["\\]/g, '\\$&');
  }
  function escapeHtml(value) {
    return String(value).replace(/[&<>"']/g, (ch) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[ch]));
  }
  function escapeAttr(value) { return escapeHtml(value); }
  function isEditing() {
    const active = document.activeElement;
    return !!active && (
      active.matches?.('input, textarea, select') ||
      active.isContentEditable ||
      ['search', 'accessPassword', 'userInput'].includes(active.id)
    );
  }

  window.addEventListener('beforeunload', () => {
    if (pollHandle) clearTimeout(pollHandle);
  });
  init();
}());
