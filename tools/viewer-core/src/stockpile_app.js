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
  let lastSyncText = '-';
  let syncError = '';
  let pollHandle = null;
  let participantRegistered = false;
  let config = defaultConfig();
  let authStatus = { authenticated: false, admin: false, access_password_enabled: false, whitelist_enabled: false, allow_guest_readonly: false };
  let searchRenderTimer = null;
  let searchComposing = false;

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
      myTasks: '我的任务', myPreparing: '我备货中', myDone: '我已完成', myTotal: '我参与总数',
      overfilledOnly: '超额材料', stalledOnly: '已备货未完成', lockedOnly: '只看锁定', notedOnly: '只看有备注',
      copyAllGaps: '复制全部缺口', copyMine: '复制我的任务', copyUnclaimed: '复制无人认领', copyOverfilled: '复制超额材料',
      copied: '已复制', readonlyMode: '只读模式', recentBy: '最近修改', publicNote: '备注', storageLocation: '存放位置',
      locked: '锁定', materialLocked: '材料已锁定', loginRequired: '需要先输入访问密码', readonlyGuest: '只读访客，不能修改',
      accessPassword: '访问密码'
    },
    'en-US': {
      myTasks: 'My tasks', myPreparing: 'My preparing', myDone: 'My done', myTotal: 'My total',
      overfilledOnly: 'Overfilled', stalledOnly: 'Prepared not done', lockedOnly: 'Locked only', notedOnly: 'With notes',
      copyAllGaps: 'Copy all gaps', copyMine: 'Copy my tasks', copyUnclaimed: 'Copy unclaimed', copyOverfilled: 'Copy overfilled',
      copied: 'Copied', readonlyMode: 'Read-only mode', recentBy: 'Last changed by', publicNote: 'Note', storageLocation: 'Storage',
      locked: 'Locked', materialLocked: 'Material is locked', loginRequired: 'Access password required', readonlyGuest: 'Read-only guest',
      accessPassword: 'Access password'
    }
  };
  function dict() { return (data.i18n && data.i18n[lang]) || (data.i18n && data.i18n['en-US']) || {}; }
  function t(key) { return dict()[key] || extraI18n[lang]?.[key] || extraI18n['en-US'][key] || defaultLabel(key) || key; }
  function defaultLabel(key) {
    const zh = {
      totalQuantity: '总数量', rawCount: '原始个数', stackSize: '每组数量', unitChest: '箱', unitBox: '盒', unitStack: '组', unitEach: '个',
      direct: '可采集', recipeDirect: '可直接采集/挖掘获得', namespaceId: '命名空间 ID', stacksRemainder: '组 / 余数', shulkerBoxes: '盒数',
      tagPlanks: '任意木板', tagLogs: '任意原木', tagStone: '任意石材', tagCoals: '任意煤炭', tagIronOres: '任意铁矿石'
    };
    const en = {
      totalQuantity: 'Total quantity', rawCount: 'Raw count', stackSize: 'Stack size', unitChest: 'chest', unitBox: 'box', unitStack: 'stack', unitEach: 'item',
      direct: 'Gatherable', recipeDirect: 'Directly gather or mine this material', namespaceId: 'Namespace ID', stacksRemainder: 'stacks / remainder', shulkerBoxes: 'boxes',
      tagPlanks: 'Any planks', tagLogs: 'Any logs', tagStone: 'Any stone material', tagCoals: 'Any coal', tagIronOres: 'Any iron ore'
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
    await refreshState();
    render();
    ensureUser();
    if (isServeMode) {
      pollHandle = setInterval(async () => {
        if (userId && !participantRegistered) await registerParticipant();
        await refreshState();
        if (document.activeElement?.id === 'search' || searchComposing) return;
        render();
      }, Math.min(10000, Math.max(2000, Number(config.poll_interval_ms || 3000))));
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
  async function refreshState() {
    if (!isServeMode) {
      syncState = offlineSyncState();
      return;
    }
    try {
      syncState = await apiGet('/api/state');
      try { authStatus = await apiGet('/api/auth/status'); } catch (_) {}
      participantRegistered = !!userId && (syncState.participants || []).some((participant) => participant.user_id === userId);
      lastSyncText = new Date().toLocaleTimeString();
      syncError = '';
    } catch (error) {
      syncError = `${t('syncError')}: ${error.message}`;
    }
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
        preparing_count: preparing,
        done_count: done,
        remaining_count: Math.max(0, item.required_count - total),
        overfilled_count: Math.max(0, total - item.required_count),
        participants: local.assignee ? [local.assignee] : [],
        overall_status: total === 0 ? 'not_started' : total > item.required_count ? 'overfilled' : done >= item.required_count ? 'done' : done > 0 ? 'partial_done' : 'preparing',
        claims: local.assignee ? [{ user_id: local.assignee, status: status === 'done' ? 'done' : 'preparing', quantity }] : []
      };
    }
    return withClientSummaries({ participants: userId ? [{ user_id: userId }] : [], materials, updated_at: Date.now() });
  }

  function materialSync(item) {
    return syncState.materials[item.namespace_id] || {
      material_id: item.namespace_id,
      required_count: item.required_count,
      preparing_count: 0,
      done_count: 0,
      remaining_count: item.required_count,
      overfilled_count: 0,
      participants: [],
      overall_status: 'not_started',
      claims: []
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
    return `<section class="toolbar"><div class="password-row"><input class="field" id="accessPassword" type="password" placeholder="${escapeAttr(t('accessPassword') || 'Access password')}" /><button class="button icon-button" id="toggleAccessPassword" type="button" title="${escapeAttr(t('accessPassword') || 'Access password')}">👁</button></div><button class="button primary" id="accessLogin">${escapeHtml(t('enter') || 'Enter')}</button></section>`;
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
      await refreshState();
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
        case 'noted': return !!(sync.public_note || sync.storage_location);
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
      if (sync.public_note || sync.storage_location) summaries.noted_materials.push(id);
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
        ${isServeMode ? `<span class="badge">${escapeHtml(t('lastSync'))}: ${escapeHtml(lastSyncText)}</span>${config.mode === 'multi' ? `<span class="badge">${escapeHtml(t('participants'))}: ${(syncState.participants || []).length}</span>` : ''}` : ''}
        <select class="field" id="lang" aria-label="${escapeAttr(t('language'))}"><option value="zh-CN" ${lang === 'zh-CN' ? 'selected' : ''}>中文</option><option value="en-US" ${lang === 'en-US' ? 'selected' : ''}>English</option></select>
        <button class="button" id="switchUser">${escapeHtml(t('switchId'))}</button>
      </div>
    </header>
    <main class="shell">
      ${syncError ? `<div class="empty">${escapeHtml(syncError)}</div>` : ''}
      <section class="summary">
        ${metric(t('totalMaterials'), data.materials.summary.unique_materials)}
        ${metric(t('totalQuantity'), formatQuantity(data.materials.summary.total_blocks))}
        ${metric(t('shulkerEstimate'), `${data.materials.summary.estimated_shulker_boxes}${t('unitBox')}`)}
        <div class="metric"><b>${allTotals.progress}%</b><span>${escapeHtml(t('done'))}</span><div class="progress-track"><div class="progress-bar" style="width:${allTotals.progress}%"></div></div></div>
      </section>
      <section class="summary">
        ${metric(t('myPreparing'), `${mine.preparing_count} / ${formatQuantity(mine.preparing_quantity)}`)}
        ${metric(t('myDone'), `${mine.done_count} / ${formatQuantity(mine.done_quantity)}`)}
        ${metric(t('myTotal'), mine.material_count)}
        ${metric(t('unclaimed'), stateSummaries().unclaimed_materials?.length || 0)}
      </section>
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
      ['locked', t('lockedOnly')], ['noted', t('notedOnly')], ['available', t('craftable')], ['unresolved', t('unresolved')], ['missing', t('recipeMissing')]
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
      <div class="section-head"><div class="section-icon">${iconImg(icon, true)}</div><div><h2>${escapeHtml(group.category)}</h2><div class="section-meta">${group.items.length} · ${escapeHtml(t('required'))} ${info.required} · ${escapeHtml(t('claimed'))} ${info.assigned}</div></div><div class="section-rate">${info.progress}%</div></div>
      <div class="list">${group.items.map(renderCard).join('')}</div>
    </section>`;
  }
  function renderCard(item) {
    const sync = materialSync(item);
    const claim = myClaim(item);
    const isOpen = open.has(item.namespace_id);
    const disabled = !canWrite(item);
    const hint = writeHint(item);
    return `<article class="card ${isOpen ? 'open' : ''}" data-id="${escapeAttr(item.namespace_id)}">
      <div class="card-main">
        <div>
          <div class="material-title">${iconImg(item.item_icon_key, true, item)}<span class="name">${escapeHtml(displayName(item))}</span></div>
          <div class="sub">${escapeHtml(item.namespace_id)} · ${escapeHtml(item.category)} · ${escapeHtml(t('remaining'))} ${sync.remaining_count}</div>
          <div class="badges"><span class="badge ${item.recipe_status}">${recipeLabel(item.recipe_status)}</span><span class="badge ${sync.overall_status}">${overallLabel(sync.overall_status)}</span><span class="badge">${sync.participants.length ? `${escapeHtml(t('claimedBy'))}: ${escapeHtml(sync.participants.join(', '))}` : escapeHtml(t('unclaimed'))}</span>${sync.locked ? `<span class="badge missing">${escapeHtml(t('locked') || 'Locked')}</span>` : ''}${config.show_icon_fallback_badge && item.icon_available === false ? `<span class="badge missing">${escapeHtml(t('iconFallback'))}</span>` : ''}</div>
          ${(sync.public_note || sync.storage_location || sync.updated_by || hint) ? `<div class="sub">${sync.public_note ? `${escapeHtml(t('publicNote'))}: ${escapeHtml(sync.public_note)} ` : ''}${sync.storage_location ? `${escapeHtml(t('storageLocation'))}: ${escapeHtml(sync.storage_location)} ` : ''}${sync.updated_by ? `${escapeHtml(t('recentBy'))}: ${escapeHtml(sync.updated_by)} ` : ''}${hint ? `${escapeHtml(hint)}` : ''}</div>` : ''}
        </div>
        <div><div class="count">${formatQuantity(item.required_count, item)}</div><div class="actions">
          <input class="field qty" type="number" min="0" value="${Number(claim.quantity || 0)}" data-action="qty" ${disabled ? 'disabled' : ''} />
          <button class="button" data-action="progress" ${disabled ? 'disabled' : ''}>${escapeHtml(t('preparing'))}</button>
          <button class="button primary" data-action="done" ${disabled ? 'disabled' : ''}>${escapeHtml(t('done'))}</button>
          <button class="button danger" data-action="cancel" ${disabled ? 'disabled' : ''}>${escapeHtml(t('cancel'))}</button>
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
    </div>
    <div class="claim-list">${(sync.claims || []).map((claim) => `<span class="claim-chip">${escapeHtml(claim.user_id)} · ${escapeHtml(overallLabel(claim.status))} · ${claim.quantity}</span>`).join('')}</div>
    ${tree && config.show_advanced_recipe_tree ? `<div class="craft-chain">${renderRecipeTree(tree, true)}</div>` : ''}${recipeNote && config.show_unresolved_recipes ? `<p class="sub">${escapeHtml(recipeNote)}</p>` : ''}`;
  }
  function renderRecipeTree(node, isRoot = true) {
    const nodeId = node.node_id || `${node.item_id}-${node.depth || 0}`;
    const collapsed = collapsedTree.has(nodeId);
    const hasChildren = (node.children || []).length > 0;
    const children = hasChildren && !collapsed ? (node.children || []).map((child) => renderRecipeTree(child, false)).join('') : '';
    const ingredients = (node.ingredients || []).map((ingredient) => `${itemName(ingredient.item_id, ingredient.display_name || ingredient.item_id)} x${ingredient.needed_count}${ingredient.unresolved ? ` (${reasonLabel(ingredient.unresolved_reason)})` : ''}`).join(' · ');
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
              <span class="recipe-badge">${escapeHtml(t('need'))} ${node.needed_count}</span>
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
    return `<span class="icon-frame ${large ? 'large' : ''}"><img class="icon-img" src="${escapeAttr(src)}" alt="" loading="lazy" /></span>`;
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
  function formatQuantity(count, item) {
    let remaining = Math.max(0, Number(count || 0));
    const stackSize = item ? effectiveStackSize(item) : 64;
    const chestSize = stackSize * 54;
    const boxSize = stackSize * 27;
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

  function bindControls() {
    document.getElementById('switchUser').addEventListener('click', () => {
      localStorage.removeItem(userKey);
      userId = '';
      participantRegistered = false;
      ensureUser();
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
        await refreshState();
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
      card.querySelector('[data-action="progress"]').addEventListener('click', () => updateClaim(item, 'preparing'));
      card.querySelector('[data-action="done"]').addEventListener('click', () => updateClaim(item, 'done'));
      card.querySelector('[data-action="cancel"]').addEventListener('click', () => deleteClaim(item));
      card.querySelector('[data-action="toggle"]').addEventListener('click', () => { open.has(item.namespace_id) ? open.delete(item.namespace_id) : open.add(item.namespace_id); render(); });
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
      await refreshState();
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
      await refreshState();
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
      lines.push(`- ${displayName(item)} (${item.namespace_id}) x${count} [${overallLabel(sync.overall_status)}]`);
    }
    return lines.join('\n');
  }
  async function copyText(text) {
    try {
      if (navigator.clipboard?.writeText) await navigator.clipboard.writeText(text);
      else {
        const area = document.createElement('textarea');
        area.value = text;
        document.body.appendChild(area);
        area.select();
        document.execCommand('copy');
        area.remove();
      }
      syncError = t('copied');
    } catch (error) {
      syncError = `${t('syncError')}: ${error.message}`;
    }
    render();
  }
  function cssEscape(value) {
    return String(value).replace(/["\\]/g, '\\$&');
  }
  function escapeHtml(value) {
    return String(value).replace(/[&<>"']/g, (ch) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[ch]));
  }
  function escapeAttr(value) { return escapeHtml(value); }

  window.addEventListener('beforeunload', () => {
    if (pollHandle) clearInterval(pollHandle);
  });
  init();
}());
