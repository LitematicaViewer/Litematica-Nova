(function () {
  let data = window.__STOCKPILE_DATA__;
  const app = document.getElementById('app');
  const isServeMode = location.protocol === 'http:' || location.protocol === 'https:';
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
  function dict() { return (data.i18n && data.i18n[lang]) || (data.i18n && data.i18n['en-US']) || {}; }
  function t(key) { return dict()[key] || key; }
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
      } catch (error) {
        syncError = `${t('syncError')}: ${error.message}`;
      }
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
    return { participants: userId ? [{ user_id: userId }] : [], materials, updated_at: Date.now() };
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

  function render() {
    document.title = t('appTitle');
    const items = filteredMaterials();
    const allTotals = totals(data.materials.materials);
    app.innerHTML = `<header class="topbar">
      <div class="brand"><strong>${escapeHtml(t('appTitle'))}</strong><span>${escapeHtml(data.manifest.source_file)}</span></div>
      <div class="identity">
        <span class="note">${escapeHtml(isServeMode ? t('syncMode') : t('offlineMode'))}</span>
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
        ${metric(t('totalBlocks'), data.materials.summary.total_blocks)}
        ${metric(t('totalStacks'), data.materials.summary.total_stacks)}
        <div class="metric"><b>${allTotals.progress}%</b><span>${escapeHtml(t('done'))}</span><div class="progress-track"><div class="progress-bar" style="width:${allTotals.progress}%"></div></div></div>
      </section>
      <section class="toolbar">
        <input class="field" id="search" value="${escapeAttr(controls.search)}" placeholder="${escapeAttr(t('search'))}" />
        <select class="field" id="sort">${sortOptions()}</select>
        <select class="field" id="filter">${filterOptions()}</select>
      </section>
      ${renderList(items)}
    </main>`;
    bindControls();
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
      ['mine', t('mine')], ['unclaimed', t('unclaimed')], ['available', t('craftable')], ['unresolved', t('unresolved')], ['missing', t('recipeMissing')]
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
    return `<article class="card ${isOpen ? 'open' : ''}" data-id="${escapeAttr(item.namespace_id)}">
      <div class="card-main">
        <div>
          <div class="material-title">${iconImg(item.item_icon_key, true, item)}<span class="name">${escapeHtml(displayName(item))}</span></div>
          <div class="sub">${escapeHtml(item.namespace_id)} · ${escapeHtml(item.category)} · ${escapeHtml(t('remaining'))} ${sync.remaining_count}</div>
          <div class="badges"><span class="badge ${item.recipe_status}">${recipeLabel(item.recipe_status)}</span><span class="badge ${sync.overall_status}">${overallLabel(sync.overall_status)}</span><span class="badge">${sync.participants.length ? `${escapeHtml(t('claimedBy'))}: ${escapeHtml(sync.participants.join(', '))}` : escapeHtml(t('unclaimed'))}</span>${config.show_icon_fallback_badge && item.icon_available === false ? `<span class="badge missing">${escapeHtml(t('iconFallback'))}</span>` : ''}</div>
        </div>
        <div><div class="count">${item.required_count}</div><div class="actions">
          <input class="field qty" type="number" min="0" value="${Number(claim.quantity || 0)}" data-action="qty" />
          <button class="button" data-action="progress">${escapeHtml(t('preparing'))}</button>
          <button class="button primary" data-action="done">${escapeHtml(t('done'))}</button>
          <button class="button danger" data-action="cancel">${escapeHtml(t('cancel'))}</button>
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
      ${detail('namespace_id', item.namespace_id)}
      ${detail(t('quantity'), item.required_count)}
      ${detail('stacks / remainder', `${item.stacks} / ${item.remainder}`)}
      ${detail('shulker_boxes', item.shulker_boxes)}
      ${detail(t('sourceRegions'), (item.source_regions || []).join(', ') || '-')}
      ${detail(t('recipeStatus'), item.recipe_status)}
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
    const iconKey = node.visual_kind === 'tag' ? '__tag' : node.visual_kind === 'special' ? '__special' : node.unresolved ? '__unresolved' : node.icon_key;
    return `<div class="tree-node ${isRoot ? 'root' : ''}" data-tree-id="${escapeAttr(nodeId)}">
      <div class="recipe-card ${escapeAttr(node.visual_kind || '')}">
        <div class="recipe-card-head">
          ${iconImg(iconKey, true)}
          <div>
            <div class="recipe-name">${escapeHtml(itemName(node.item_id, node.display_name || node.item_id))}</div>
            <div class="recipe-id">${escapeHtml(node.tag || node.item_id)}</div>
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
      'minecraft:smithing_trim': 'recipeSmithingTrim'
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
    if (node.visual_kind === 'tag') return `${t('tagGroup')} ${node.tag || node.item_id}`;
    if (node.visual_kind === 'special') return `${t('specialRecipe')}: ${node.recipe_type}`;
    if (node.unresolved_reason === 'no_recipe') return t('noRecipe');
    if (node.unresolved_reason === 'tag_input') return t('tagInput');
    if (node.unresolved) return `${t('unresolved')}: ${reasonLabel(node.unresolved_reason)}`;
    return '';
  }
  function detail(label, value) { return `<div class="detail"><span>${escapeHtml(label)}</span>${escapeHtml(String(value))}</div>`; }
  function recipeLabel(status) {
    return { available: t('available'), unresolved: t('unresolved'), missing: t('missing') }[status] || status;
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
    return data.item_names?.names?.[itemId]?.[lang] || data.item_names?.names?.[itemId]?.['en-US'] || fallback || itemId;
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
    document.getElementById('search').addEventListener('input', (event) => { controls.search = event.target.value; render(); });
    document.getElementById('sort').addEventListener('change', (event) => { controls.sort = event.target.value; render(); });
    document.getElementById('filter').addEventListener('change', (event) => { controls.filter = event.target.value; render(); });
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
