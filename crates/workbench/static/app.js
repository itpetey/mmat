let state = { project: {}, projects: [], roles: {}, events: [], messages: [], artefacts: [], memories: [], notifications: [], dag_steps: [], active_artefact_id: null, active_step_id: null };
let selectedArtefactId = null;
let selectedStepId = null;
let selectedRoleId = null;
let activeView = 'chat';

let selectedEventId = null;
let eventFilters = { role: '', variant: '', run: '', task: '', lane: '' };

async function loadState() {
  const response = await fetch('/api/state');
  state = await response.json();
  render();
}

function connectEvents() {
  const source = new EventSource('/events');
  source.onopen = () => {
    setConnectionStatus('connected', 'Connected');
    loadState();
  };
  source.onerror = () => {
    if (source.readyState === EventSource.CONNECTING) {
      setConnectionStatus('reconnecting', 'Reconnecting…');
    } else {
      setConnectionStatus('disconnected', 'Disconnected. Refresh to reconnect.');
    }
  };
  source.onmessage = (message) => {
    const update = JSON.parse(message.data);
    if (update.type === 'State') {
      state = update.payload;
      render();
    }
    if (update.type === 'Event') {
      state.events.push(update.payload);
      if (state.events.length > 200) state.events = state.events.slice(-200);
      loadState();
    }
    if (update.type === 'Notice') console.warn(update.payload);
  };
}

function setConnectionStatus(cls, text) {
  const el = document.getElementById('connection-status');
  if (!el) return;
  el.className = 'connection-status ' + cls;
  el.textContent = text;
}

function render() {
  renderHeader();
  renderProjects();
  renderRunControls();
  renderNextAction();
  renderConversation();
  renderRoleReadiness();
  renderDag();
  renderStepDetail();
  renderEvents();
  renderNotifications();
}

function renderHeader() {
  const project = state.project || {};
  document.getElementById('project-chip').textContent = project.name || 'SELIUM';
  document.getElementById('events-view-button').classList.toggle('active', activeView === 'events');
  document.getElementById('chat-view-button').classList.toggle('active', activeView === 'chat');
  document.getElementById('dag-view-button').classList.toggle('active', activeView === 'dag');
  document.getElementById('events-view').classList.toggle('active', activeView === 'events');
  document.getElementById('chat-view').classList.toggle('active', activeView === 'chat');
  document.getElementById('dag-view').classList.toggle('active', activeView === 'dag');
}

function renderProjects() {
  const list = document.getElementById('project-list');
  if (!list) return;
  const projects = state.projects || [];
  const activeId = state.active_project_id || '';
  list.innerHTML = '';
  if (!projects.length) {
    list.innerHTML = '<li class="empty">No projects yet.</li>';
    return;
  }
  for (const project of projects) {
    const li = document.createElement('li');
    li.className = 'project-item' + (project.id === activeId ? ' active' : '');
    li.tabIndex = 0;
    li.setAttribute('role', 'button');
    li.setAttribute('aria-pressed', project.id === activeId ? 'true' : 'false');
    li.innerHTML = `<span class="project-name">${escapeHtml(project.name || project.id)}</span>`;

    const actions = document.createElement('span');
    actions.style.display = 'flex';
    actions.style.gap = '4px';

    const renameBtn = document.createElement('button');
    renameBtn.type = 'button';
    renameBtn.textContent = 'Rename';
    renameBtn.setAttribute('aria-label', `Rename project ${project.name || project.id}`);
    renameBtn.onclick = (e) => { e.stopPropagation(); promptRenameProject(project.id); };
    renameBtn.onkeydown = (e) => { if (e.key === 'Enter' || e.key === ' ') { e.stopPropagation(); promptRenameProject(project.id); } };

    const deleteBtn = document.createElement('button');
    deleteBtn.type = 'button';
    deleteBtn.textContent = 'Delete';
    deleteBtn.className = 'danger';
    deleteBtn.setAttribute('aria-label', `Delete project ${project.name || project.id}`);
    deleteBtn.onclick = (e) => { e.stopPropagation(); confirmDeleteProject(project.id); };
    deleteBtn.onkeydown = (e) => { if (e.key === 'Enter' || e.key === ' ') { e.stopPropagation(); confirmDeleteProject(project.id); } };

    actions.appendChild(renameBtn);
    actions.appendChild(deleteBtn);
    li.appendChild(actions);

    li.onclick = () => selectProject(project.id);
    li.onkeydown = (e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); selectProject(project.id); } };
    list.appendChild(li);
  }
}

async function selectProject(id) {
  if (id === state.active_project_id) return;
  await fetch(`/api/projects/${encodeURIComponent(id)}/select`, { method: 'POST' });
  await loadState();
}

function promptRenameProject(id) {
  const project = (state.projects || []).find(p => p.id === id);
  if (!project) return;
  const newName = prompt(`Rename project "${project.name || project.id}" to:`, project.name || project.id);
  if (!newName || newName.trim() === (project.name || project.id)) return;
  fetch(`/api/projects/${encodeURIComponent(id)}`, {
    method: 'PATCH',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ name: newName.trim() })
  }).then(() => loadState());
}

function confirmDeleteProject(id) {
  const project = (state.projects || []).find(p => p.id === id);
  if (!project) return;
  if (!confirm(`Delete project "${project.name || project.id}"? This cannot be undone.`)) return;
  fetch(`/api/projects/${encodeURIComponent(id)}`, { method: 'DELETE' }).then(() => loadState());
}

document.getElementById('new-project-button').addEventListener('click', async () => {
  const name = prompt('Project name:');
  if (!name || !name.trim()) return;
  await fetch('/api/projects', {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ name: name.trim() })
  });
  await loadState();
});

function renderRunControls() {
  const runLabel = document.getElementById('run-label');
  const runs = state.runs || [];
  const activeRun = runs.find(r => r.id === state.active_run_id) || runs[0];
  if (runLabel) runLabel.textContent = activeRun ? (activeRun.label || activeRun.id) : 'No run';
  const selector = document.getElementById('run-selector');
  if (selector) {
    selector.innerHTML = runs.map(run => `<option value="${escapeHtml(run.id)}"${run.id === state.active_run_id ? ' selected' : ''}>${escapeHtml(run.label || run.id)} · ${escapeHtml(run.status)}</option>`).join('');
  }
}

function renderNextAction() {
  const root = document.getElementById('next-action');
  if (!root) return;
  const events = state.events || [];
  const steps = state.dag_steps || [];
  const pendingReqs = (state.action_requests || []).filter(r => r.status === 'pending');
  const pendingNotifs = (state.notifications || []).filter(n => !n.acknowledged);
  const runningSteps = steps.filter(s => s.state === 'Running');
  const failedSteps = steps.filter(s => s.state === 'Failed' || s.state === 'Needs rework');
  const hasConversation = state.has_conversation || events.length > 0;

  if (!hasConversation) {
    root.innerHTML = '<strong>Welcome to MMAT.</strong> Ask a question or describe a project goal to begin. Mention <span class="action-hint">@intent</span> to refine direction, <span class="action-hint">@scholar</span> to research, or <span class="action-hint">@architect</span> to design.';
    return;
  }

  if (pendingReqs.length > 0) {
    const req = pendingReqs[0];
    root.innerHTML = `<strong>Action needed</strong> — <span class="action-count">${pendingReqs.length} pending</span>${req.prompt ? `: ${escapeHtml(req.prompt)}` : ''}. Reply inline or type in the composer below.`;
    return;
  }

  if (failedSteps.length > 0) {
    root.innerHTML = `<strong>Attention required</strong> — ${failedSteps.length} step${failedSteps.length > 1 ? 's' : ''} ${failedSteps.map(s => escapeHtml(s.label)).join(', ')} ${failedSteps.length > 1 ? 'need' : 'needs'} attention. Check the <a href="#" onclick="activeView='dag';renderHeader();renderDag();renderStepDetail();return false;">DAG view</a> for details.`;
    return;
  }

  if (runningSteps.length > 0) {
    root.innerHTML = `<strong>In progress</strong> — <span class="action-count">${runningSteps.length} step${runningSteps.length > 1 ? 's' : ''}</span> running: ${runningSteps.map(s => escapeHtml(s.label)).join(', ')}. Check the <a href="#" onclick="activeView='dag';renderHeader();renderDag();renderStepDetail();return false;">DAG view</a> for progress.`;
    return;
  }

  if (pendingNotifs.length > 0) {
    root.innerHTML = `<strong>Notifications</strong> — <span class="action-count">${pendingNotifs.length}</span> item${pendingNotifs.length > 1 ? 's' : ''} waiting. Click the notification badge to review.`;
    return;
  }

  root.innerHTML = '<strong>No pending actions.</strong> Everything is up to date. Start a new message or check the <a href="#" onclick="activeView=\'dag\';renderHeader();renderDag();renderStepDetail();return false;">DAG</a> for delivery status.';
}

function renderConversation() {
  const root = document.getElementById('conversation');
  root.innerHTML = '';
  const entries = channelEntries();
  for (const entry of entries) {
    const el = document.createElement('div');
    el.className = 'channel-row';
    el.innerHTML = `<div class="speaker-label ${speakerClass(entry.speaker)}">${escapeHtml(entry.speaker)}</div><div class="message-body ${entry.kind}">${formatMessage(entry.content)}</div>`;
    root.appendChild(el);
  }
  root.scrollTop = root.scrollHeight;
}

function channelEntries() {
  const eventEntries = (state.events || []).filter(isChannelEvent).map(event => {
    const detail = event.detail || {};
    switch (event.variant) {
      case 'HumanFeedbackReceived':
        return { speaker: 'ME', kind: 'text', content: detail.answer || event.summary, timestamp_ns: event.timestamp_ns };
      case 'HumanFeedbackRequested':
        return { speaker: roleName(event.source_agent), kind: 'text', content: `@me ${detail.question || event.summary}`, timestamp_ns: event.timestamp_ns };
      case 'ToolExecuted':
        return { speaker: roleName(event.source_agent), kind: 'log', content: toolText(detail), timestamp_ns: event.timestamp_ns };
      case 'ClaimMade':
        return { speaker: roleName(event.source_agent), kind: 'text', content: detail.claim_text || event.summary, timestamp_ns: event.timestamp_ns };
      case 'DecisionRecorded':
        return { speaker: roleName(event.source_agent), kind: 'text', content: detail.decision_text || event.summary, timestamp_ns: event.timestamp_ns };
      case 'ArtefactProduced':
        return { speaker: roleName(event.source_agent), kind: 'system', content: `Produced ${detail.artefact_type || 'artefact'} ${detail.artefact_id || ''}`, timestamp_ns: event.timestamp_ns };
      case 'ReviewCompleted':
        return { speaker: roleName(event.source_agent), kind: 'system', content: event.summary, timestamp_ns: event.timestamp_ns };
      default:
        return { speaker: roleName(event.source_agent), kind: 'system', content: event.summary, timestamp_ns: event.timestamp_ns };
    }
  });
  const handoffEntries = (state.messages || [])
    .filter(message => String(message.speaker || '').startsWith('System ('))
    .map(message => ({ speaker: message.speaker, kind: 'system', content: message.content, timestamp_ns: message.timestamp_ns }));
  return eventEntries.concat(handoffEntries).sort((a, b) => (a.timestamp_ns || 0) - (b.timestamp_ns || 0));
}

function isChannelEvent(event) {
  return !['OrganisationStarted', 'OrganisationStopped', 'RoleStateChanged', 'Heartbeat'].includes(event.variant);
}

function renderRoleReadiness() {
  const root = document.getElementById('role-readiness');
  if (!root) return;
  const roles = state.roles || {};
  const entries = Object.entries(roles);
  if (!entries.length) {
    root.innerHTML = '<div class="empty">No role information yet.</div>';
    return;
  }
  root.innerHTML = '';
  for (const [roleId, role] of entries) {
    const r = role.readiness || {};
    const capability = r.capability || 'fallback';
    const label = role.label || 'Role';
    const badge = document.createElement('button');
    badge.type = 'button';
    badge.className = `role-badge${selectedRoleId === roleId ? ' active' : ''}`;
    badge.onclick = () => { selectedRoleId = roleId; selectedStepId = null; renderRoleReadiness(); renderRoleDetail(); };
    badge.innerHTML = `<span class="status-dot ${escapeHtml(capability)}" title="${escapeHtml(capability)}"></span><span class="role-name">${escapeHtml(label)}</span><span class="role-state ${roleStateClass(role.state)}">${escapeHtml(role.state || 'idle')}</span>${roleActivity(role.state)}`;
    root.appendChild(badge);
  }
  if (selectedRoleId) renderRoleDetail();
}

function renderDag() {
  const root = document.getElementById('dag');
  const steps = state.dag_steps || [];
  const activeId = selectedStepId || state.active_step_id || (steps[0] && steps[0].id);
  root.innerHTML = steps.length ? '' : '<div class="empty">No project flow yet.</div>';
  for (const step of steps) {
    const button = document.createElement('button');
    button.type = 'button';
    button.className = `dag-step ${step.id === activeId ? 'active' : ''}`;
    button.onclick = () => { selectedStepId = step.id; selectedRoleId = null; renderRoleReadiness(); renderDag(); renderStepDetail(); };
    button.innerHTML = `<strong>${escapeHtml(step.label)}</strong><div class="state ${dagStateClass(step.state)}"><span class="step-status ${dagStateClass(step.state)}"></span>${escapeHtml(step.state)} · ${escapeHtml(step.role)}</div><div>${escapeHtml(step.summary)}</div>`;
    root.appendChild(button);
  }
}

function renderStepDetail() {
  if (selectedRoleId) {
    renderRoleDetail();
    return;
  }
  const root = document.getElementById('step-detail');
  const steps = state.dag_steps || [];
  const step = steps.find(s => s.id === (selectedStepId || state.active_step_id)) || steps[0];
  if (!step) {
    root.innerHTML = '<div class="empty">Select a step to inspect artefacts, logs and semantic evidence.</div>';
    return;
  }
  const artefacts = (state.artefacts || []).filter(a => (step.artefact_ids || []).includes(a.id));
  const events = (state.events || []).filter(e => (step.event_ids || []).includes(e.id));
  const stateLower = String(step.state || '').toLowerCase();
  const stateBanner = stateLower === 'failed' || stateLower.includes('rework')
    ? `<div class="error-banner">Step is <strong>${escapeHtml(step.state)}</strong>. Inspect related events and artefacts for details.</div>`
    : '';
  root.innerHTML = `
    ${stateBanner}
    <div class="detail-card"><h3>${escapeHtml(step.label)}</h3><div>${escapeHtml(step.summary)}</div><div class="empty">Role: ${escapeHtml(step.role)} · State: <strong class="${dagStateClass(step.state)}">${escapeHtml(step.state)}</strong></div></div>
    <div class="detail-grid">
      <div class="detail-card"><h3>Artefacts</h3>${artefactsHtml(artefacts)}</div>
      <div class="detail-card"><h3>Logs</h3>${eventsHtml(events)}</div>
      <div class="detail-card"><h3>Memory</h3>${memoriesHtml(state.memories || [])}</div>
      <div class="detail-card"><h3>CoT</h3><p class="empty">Raw chain-of-thought is intentionally not shown. MMAT exposes consequential semantic events, claims, artefacts and evidence instead.</p></div>
    </div>
  `;
}

function renderRoleDetail() {
  const root = document.getElementById('step-detail');
  const roles = state.roles || {};
  const role = roles[selectedRoleId];
  if (!role) {
    root.innerHTML = '<div class="empty">Role not found in current state.</div>';
    return;
  }
  const r = role.readiness || {};
  const capability = r.capability || 'unknown';
  const roleState = role.state || 'Idle';
  const roleStateLower = String(roleState).toLowerCase();
  const relatedSteps = (state.dag_steps || []).filter(s => s.role === selectedRoleId);
  const relatedEvents = (state.events || []).filter(e => {
    const eventIds = relatedSteps.flatMap(s => s.event_ids || []);
    return eventIds.includes(e.id);
  });

  const stateBanner = roleStateLower === 'failed' || roleStateLower === 'error'
    ? `<div class="error-banner">Role is in a <strong>${escapeHtml(roleState)}</strong> state. Check DAG steps for related failures.</div>`
    : '';

  root.innerHTML = `
    ${stateBanner}
    <div class="detail-card"><h3>${escapeHtml(role.label || selectedRoleId)} — Readiness</h3></div>
    <div class="detail-grid">
      <div class="detail-card">
        <div class="meta">Operational State: <strong class="${roleStateClass(roleState)}">${escapeHtml(roleState)}</strong></div>
        <div class="meta">Capability: <span class="status-dot ${escapeHtml(capability)}" style="display:inline-block;vertical-align:middle;margin-right:4px;"></span>${escapeHtml(capability)}</div>
        <div>${escapeHtml(role.summary || '')}</div>
      </div>
      ${relatedSteps.length ? `
      <div class="detail-card">
        <h3>Related DAG Steps</h3>
        <ul class="compact-list">${relatedSteps.map(s => `<li><a href="#" onclick="selectedStepId='${escapeHtml(s.id)}';selectedRoleId=null;renderDag();renderStepDetail();renderRoleReadiness();return false;">${escapeHtml(s.label)} (${escapeHtml(s.state)})</a></li>`).join('')}</ul>
      </div>` : '<div class="detail-card"><h3>Related DAG Steps</h3><p class="empty">No DAG steps for this role.</p></div>'}
      ${relatedEvents.length ? `
      <div class="detail-card">
        <h3>Related Events</h3>
        <ul class="compact-list">${relatedEvents.map(e => `<li><a href="#" onclick="selectedEventId='${escapeHtml(e.id)}';activeView='events';renderHeader();renderEvents();return false;">${escapeHtml(e.variant)}</a> <span class="meta">${escapeHtml(e.summary)}</span></li>`).join('')}</ul>
      </div>` : ''}
      <div class="detail-card">
        <h3>Capability Details</h3>
        <ul class="compact-list">
          <li>LLM client: ${r.has_llm_client ? 'configured' : 'missing'}</li>
          <li>Tools: ${r.has_tools ? r.tool_count + ' registered' : 'none'}</li>
          <li>Fallback worktree: ${r.fallback_worktree ? 'enabled' : 'disabled'}</li>
          <li>Storage access: ${r.has_artefact_store ? 'available' : 'unavailable'}</li>
          <li>Requires LLM: ${r.requires_llm ? 'yes' : 'no, operates deterministically'}</li>
        </ul>
      </div>
    </div>
  `;
}

function renderEvents() {
  const listRoot = document.getElementById('events-list');
  const detailRoot = document.getElementById('event-detail-panel');
  let events = (state.events || []).slice().reverse();

  if (eventFilters.role) events = events.filter(e => e.source_agent === eventFilters.role);
  if (eventFilters.variant) events = events.filter(e => e.variant === eventFilters.variant);
  if (eventFilters.run) events = events.filter(e => (e.detail || {}).context && e.detail.context.run_id === eventFilters.run);
  if (eventFilters.task) events = events.filter(e => (e.detail || {}).context && e.detail.context.task_id === eventFilters.task);
  if (eventFilters.lane) events = events.filter(e => {
    const lane = classifyEventLane(e.variant);
    return lane === eventFilters.lane;
  });

  if (listRoot) {
    const filterBar = buildFilterBar();
    listRoot.innerHTML = filterBar + (events.length ? '' : '<div class="empty">No matching events.</div>');
    for (const event of events) {
      const row = document.createElement('div');
      row.className = `event-row${event.id === selectedEventId ? ' active' : ''}`;
      row.tabIndex = 0;
      row.setAttribute('role', 'option');
      row.setAttribute('aria-selected', event.id === selectedEventId ? 'true' : 'false');
      row.onclick = () => { selectedEventId = event.id; renderEvents(); };
      row.onkeydown = (e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); selectedEventId = event.id; renderEvents(); } };
      row.innerHTML = `<div class="event-variant">${escapeHtml(event.variant)}</div><div class="event-meta"><span>${escapeHtml(event.source_agent)}</span><span class="event-time">${new Date(Number(event.timestamp_ns / 1000000n)).toLocaleTimeString()}</span></div><div class="event-summary">${escapeHtml(event.summary)}</div>`;
      listRoot.appendChild(row);
    }
  }
  if (detailRoot) {
    const event = events.find(e => e.id === selectedEventId);
    if (!event) {
      detailRoot.innerHTML = '<div class="empty">Select an event to inspect its details.</div>';
      return;
    }
    const detail = event.detail || {};
    const context = detail.context || {};
    const dagSteps = (state.dag_steps || []).filter(s => (s.event_ids || []).includes(event.id));
    detailRoot.innerHTML = `
      <div class="detail-card"><h3>${escapeHtml(event.variant)}</h3><div class="meta">${escapeHtml(event.id)}</div></div>
      <div class="detail-grid">
        <div class="detail-card">
          <h3>Metadata</h3>
          <ul class="compact-list">
            <li>Source: ${escapeHtml(event.source_agent)}</li>
            <li>Time: ${new Date(Number(event.timestamp_ns / 1000000n)).toLocaleString()}</li>
            <li>Project: ${escapeHtml(context.project_id || '—')}</li>
            <li>Run: ${escapeHtml(context.run_id || '—')}</li>
            <li>Task: ${escapeHtml(context.task_id || '—')}</li>
            <li>Organisation: ${escapeHtml(context.organisation_id || '—')}</li>
          </ul>
        </div>
        ${dagSteps.length ? `
        <div class="detail-card">
          <h3>DAG Steps</h3>
          <ul class="compact-list">${dagSteps.map(s => `<li><a href="#" onclick="selectedStepId='${escapeHtml(s.id)}';selectedEventId=null;activeView='dag';renderHeader();renderDag();renderStepDetail();renderRoleReadiness();return false;">${escapeHtml(s.label)} (${escapeHtml(s.state)})</a></li>`).join('')}</ul>
        </div>` : ''}
        <div class="detail-card">
          <h3>Summary</h3>
          <div>${escapeHtml(event.summary)}</div>
        </div>
        <div class="detail-card">
          <h3>Raw JSON</h3>
          <pre>${escapeHtml(JSON.stringify(detail, null, 2))}</pre>
        </div>
      </div>
    `;
  }
}

function buildFilterBar() {
  const roles = [...new Set((state.events || []).map(e => e.source_agent))].sort();
  const variants = [...new Set((state.events || []).map(e => e.variant))].sort();
  const runs = [...new Set((state.events || []).map(e => (e.detail || {}).context ? e.detail.context.run_id : ''))].filter(Boolean).sort();
  const tasks = [...new Set((state.events || []).map(e => (e.detail || {}).context ? e.detail.context.task_id : ''))].filter(Boolean).sort();
  return `<div class="event-filters" id="event-filters">
    <select data-filter="role"><option value="">All Roles</option>${roles.map(r => `<option value="${escapeHtml(r)}"${eventFilters.role === r ? ' selected' : ''}>${escapeHtml(r)}</option>`).join('')}</select>
    <select data-filter="variant"><option value="">All Types</option>${variants.map(v => `<option value="${escapeHtml(v)}"${eventFilters.variant === v ? ' selected' : ''}>${escapeHtml(v)}</option>`).join('')}</select>
    ${runs.length ? `<select data-filter="run"><option value="">All Runs</option>${runs.map(r => `<option value="${escapeHtml(r)}"${eventFilters.run === r ? ' selected' : ''}>${escapeHtml(r)}</option>`).join('')}</select>` : ''}
    ${tasks.length ? `<select data-filter="task"><option value="">All Tasks</option>${tasks.map(t => `<option value="${escapeHtml(t)}"${eventFilters.task === t ? ' selected' : ''}>${escapeHtml(t)}</option>`).join('')}</select>` : ''}
    <select data-filter="lane"><option value="">All Lanes</option><option value="conversation"${eventFilters.lane === 'conversation' ? ' selected' : ''}>Conversation</option><option value="discovery"${eventFilters.lane === 'discovery' ? ' selected' : ''}>Discovery</option><option value="delivery"${eventFilters.lane === 'delivery' ? ' selected' : ''}>Delivery</option><option value="system"${eventFilters.lane === 'system' ? ' selected' : ''}>System</option></select>
  </div>`;
}

function classifyEventLane(variant) {
  switch (variant) {
    case 'HumanFeedbackRequested': case 'HumanFeedbackReceived':
    case 'LaneCreated': case 'LaneArchived': case 'LanePaused':
    case 'ActionRequestCreated': case 'ActionRequestResolved': case 'ActionRequestCancelled':
      return 'conversation';
    case 'MemoryProposed': case 'MemoryAccepted': case 'MemoryRejected': case 'MemorySuperseded':
    case 'ToolExecuted': case 'ClaimMade': case 'DecisionRecorded':
      return 'discovery';
    case 'TaskAssigned': case 'TaskStarted': case 'TaskCompleted': case 'TaskFailed':
    case 'ReviewRequested': case 'ReviewCompleted': case 'ArtefactProduced':
      return 'delivery';
    default:
      return 'system';
  }
}

function renderNotifications() {
  const pending = (state.notifications || []).filter(n => !n.acknowledged);
  const badge = document.getElementById('notification-count');
  badge.textContent = pending.length;
  badge.hidden = pending.length === 0;
  const panel = document.getElementById('notification-panel');
  panel.innerHTML = pending.length ? '' : '<div class="empty">No items need your attention.</div>';
  for (const item of pending) {
    const el = document.createElement('div');
    el.className = 'notice';
    el.innerHTML = `<strong>${escapeHtml(item.title)}</strong><div>${escapeHtml(item.summary)}</div><button type="button">Acknowledge</button>`;
    el.querySelector('button').onclick = async () => {
      await fetch(`/api/notifications/${encodeURIComponent(item.id)}/ack`, { method: 'POST' });
      await loadState();
    };
    panel.appendChild(el);
  }
}

function dagStateClass(state) {
  const s = String(state).toLowerCase();
  if (s === 'running') return 'running';
  if (s === 'failed' || s.includes('rework')) return 'failed';
  if (s === 'done' || s === 'completed' || s === 'accepted') return 'complete';
  return 'waiting';
}

function roleStateClass(state) {
  const s = String(state).toLowerCase();
  if (s === 'running') return 'running';
  if (s === 'failed' || s === 'error') return 'failed';
  return '';
}

function roleActivity(state) {
  const s = String(state).toLowerCase();
  if (s === 'running') return '<span class="role-activity running" aria-hidden="true"></span>';
  if (s === 'failed' || s === 'error') return '<span class="role-activity failed" aria-hidden="true"></span>';
  return '';
}

function roleName(role) {
  return ({
    human: 'ME',
    'intent-lead': 'INTENT',
    'intent-lead-001': 'INTENT',
    scholar: 'SCHOLAR',
    'scholar-001': 'SCHOLAR',
    'ops-manager': 'OPS',
    'ops-manager-001': 'OPS',
    architect: 'ARCHITECT',
    'architect-001': 'ARCHITECT',
    'project-manager': 'PM',
    'pm-001': 'PM',
    worker: 'WORKER',
    'worker-001': 'WORKER',
    reviewer: 'REVIEWER',
    'reviewer-001': 'REVIEWER',
    auditor: 'AUDITOR',
    'auditor-001': 'AUDITOR',
    librarian: 'LIBRARIAN',
    coordinator: 'SYSTEM'
  })[role] || String(role || 'SYSTEM').toUpperCase();
}

function speakerClass(speaker) {
  const normalised = String(speaker).toLowerCase();
  if (normalised === 'me') return 'speaker-me';
  if (normalised === 'intent') return 'speaker-intent';
  if (normalised === 'scholar') return 'speaker-scholar';
  if (normalised === 'ops') return 'speaker-ops';
  if (normalised === 'architect') return 'speaker-architect';
  return 'speaker-muted';
}

function toolText(detail) {
  const command = detail.tool_name || 'tool';
  const output = detail.stdout ? `\n${detail.stdout}` : '';
  return `${command}${output}`;
}

function formatMessage(value) {
  const escaped = escapeHtml(value);
  const withInline = escaped
    .replace(/(@[a-zA-Z][a-zA-Z0-9_-]*)/g, '<span class="mention">$1</span>')
    .replace(/(`[^`]+`)/g, '<span class="code-token">$1</span>');
  return renderCodeBlocks(withInline);
}

function renderCodeBlocks(html) {
  const lines = html.split('\n');
  const result = [];
  let inCodeBlock = false;
  let codeLang = '';
  let codeContent = [];
  for (const line of lines) {
    const trimmed = line.trimStart();
    if (!inCodeBlock && trimmed.startsWith('```')) {
      inCodeBlock = true;
      codeLang = escapeHtml(trimmed.slice(3).trim());
      continue;
    }
    if (inCodeBlock && trimmed.startsWith('```')) {
      inCodeBlock = false;
      result.push(`<pre class="code-block"${codeLang ? ` data-lang="${codeLang}"` : ''}><code>${codeContent.join('\n')}</code></pre>`);
      codeContent = [];
      continue;
    }
    if (inCodeBlock) {
      codeContent.push(line);
    } else {
      result.push(line);
    }
  }
  if (inCodeBlock && codeContent.length) {
    result.push(`<pre class="code-block"${codeLang ? ` data-lang="${codeLang}"` : ''}><code>${codeContent.join('\n')}</code></pre>`);
  }
  return result.join('\n');
}

document.getElementById('message-form').addEventListener('submit', async (event) => {
  event.preventDefault();
  const textarea = document.getElementById('message');
  const message = textarea.value.trim();
  if (!message) return;
  textarea.value = '';
  await fetch('/api/messages', {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({
      message,
      active_step_id: selectedStepId || state.active_step_id || null,
      active_artefact_id: selectedArtefactId || state.active_artefact_id || null
    })
  });
  await loadState();
});

document.getElementById('chat-view-button').addEventListener('click', () => {
  activeView = 'chat';
  renderHeader();
});

document.getElementById('dag-view-button').addEventListener('click', () => {
  activeView = 'dag';
  renderHeader();
});

document.getElementById('events-view-button').addEventListener('click', () => {
  activeView = 'events';
  renderHeader();
});

const notifBadge = document.getElementById('notification-count');
notifBadge.addEventListener('click', (event) => {
  event.stopPropagation();
  document.getElementById('notification-panel').classList.toggle('open');
});
notifBadge.addEventListener('keydown', (event) => {
  if (event.key === 'Enter' || event.key === ' ') {
    event.preventDefault();
    event.stopPropagation();
    document.getElementById('notification-panel').classList.toggle('open');
  }
});

document.getElementById('message').addEventListener('keydown', (event) => {
  if ((event.metaKey || event.ctrlKey) && event.key === 'Enter') {
    event.preventDefault();
    document.getElementById('message-form').requestSubmit();
  }
});

function listHtml(items) {
  if (!items.length) return '<span class="empty">Not known yet.</span>';
  return `<ul class="compact-list">${items.map(item => `<li>${escapeHtml(item)}</li>`).join('')}</ul>`;
}

function artefactsHtml(artefacts) {
  if (!artefacts.length) return '<div class="empty">No artefact linked to this step yet.</div>';
  return artefacts.map(artefact => artefact.storage_kind === 'code' ? codeArtefactHtml(artefact) : blobArtefactHtml(artefact)).join('');
}

function blobArtefactHtml(artefact) {
  const content = artefact.content || {};
  const error = content.error ? `<div class="empty">${escapeHtml(content.error)}</div>` : '';
  const title = content.title ? `<div class="artefact-title">${escapeHtml(content.title)}</div>` : '';
  const description = content.description || content.summary ? `<div class="artefact-description">${escapeHtml(content.description || content.summary || '')}</div>` : '';
  return `<div class="memory"><div class="meta">Blob · ${escapeHtml(artefact.title || content.title || 'Untitled')} · ${escapeHtml(artefact.producer_role)} · ${escapeHtml(artefact.content_hash || '')}</div>${error}${title}${description}<details class="raw-payload"><summary>Show raw payload</summary><pre>${escapeHtml(JSON.stringify(content, null, 2))}</pre></details>${evidenceHtml(artefact.evidence_refs || [])}</div>`;
}

function codeArtefactHtml(artefact) {
  const output = artefact.repository_output || {};
  const content = artefact.content || {};
  const paths = output.paths || content.paths || [];
  const missing = content.missing_paths || [];
  const error = content.error ? `<div class="empty">${escapeHtml(content.error)}${missing.length ? `: ${escapeHtml(missing.join(', '))}` : ''}</div>` : '';
  const diffSummary = output.diff_summary || content.diff_summary || '';
  const validationSummary = output.validation_summary || content.validation_summary || '';
  return `<div class="memory"><div class="meta">Code · ${escapeHtml(artefact.title || output.title || 'Untitled')} · ${escapeHtml(artefact.producer_role)}</div>${error}<ul class="compact-list"><li>Repository: ${escapeHtml(output.repository_path || content.repository_path || 'unknown')}</li><li>Worktree: ${escapeHtml(output.worktree_path || content.worktree_path || 'unknown')}</li><li>Branch: ${escapeHtml(output.worktree_branch || content.worktree_branch || 'unknown')}</li>${diffSummary ? `<li><details class="raw-payload"><summary>Diff: ${escapeHtml(diffSummary.slice(0, 80))}${diffSummary.length > 80 ? '…' : ''}</summary><pre>${escapeHtml(diffSummary)}</pre></details></li>` : '<li>Diff: No diff summary</li>'}${validationSummary ? `<li>Validation: ${escapeHtml(validationSummary)}</li>` : '<li>Validation: Not run</li>'}</ul>${listHtml(paths)}${evidenceHtml(artefact.evidence_refs || [])}</div>`;
}

function evidenceHtml(refs) {
  if (!refs.length) return '<div class="empty">No evidence refs linked.</div>';
  return `<div class="meta">Evidence: ${refs.map(ref => `<a href="#event-${escapeHtml(ref)}">${escapeHtml(ref)}</a>`).join(', ')}</div>`;
}

function eventsHtml(events) {
  if (!events.length) return '<div class="empty">No logs linked to this step yet.</div>';
  return `<ul class="compact-list">${events.map(event => `<li><a href="#event-${escapeHtml(event.id)}" id="event-${escapeHtml(event.id)}"><strong>${escapeHtml(event.variant)}</strong></a> <span class="meta">${escapeHtml(event.id)}</span> ${escapeHtml(event.summary)}</li>`).join('')}</ul>`;
}

function memoriesHtml(memories) {
  if (!memories.length) return '<div class="empty">No memory candidates yet.</div>';
  return memories.slice().reverse().slice(0, 4).map(memory => `<div class="memory"><div class="meta">${escapeHtml(memory.status)} · ${escapeHtml(memory.scope)} · ${escapeHtml(memory.memory_type)}</div><div>${escapeHtml(memory.content)}</div></div>`).join('');
}

function escapeHtml(value) {
  return String(value).replace(/[&<>'"]/g, char => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '\'': '&#39;', '"': '&quot;' }[char]));
}

loadState();
connectEvents();

document.getElementById('new-run-button').addEventListener('click', async () => {
  const label = prompt('Run label (optional):') || '';
  await fetch('/api/runs', {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ label })
  });
  await loadState();
});

document.getElementById('run-selector').addEventListener('change', async (event) => {
  const runId = event.target.value;
  if (runId) {
    await fetch(`/api/runs/${encodeURIComponent(runId)}/select`, { method: 'POST' });
    await loadState();
  }
});

document.getElementById('archive-run-button').addEventListener('click', async () => {
  const activeRunId = state.active_run_id;
  if (!activeRunId) return;
  await fetch(`/api/runs/${encodeURIComponent(activeRunId)}/archive`, { method: 'POST' });
  await loadState();
});

document.getElementById('reset-project-button').addEventListener('click', async () => {
  if (!confirm('This will clear all project state (messages, events, artefacts, DAG). This cannot be undone. Continue?')) return;
  await fetch('/api/project/reset', {
    method: 'POST',
    headers: { 'X-Confirm': 'true' }
  });
  await loadState();
});

document.addEventListener('change', (event) => {
  const select = event.target;
  if (!select.closest('#event-filters')) return;
  const filter = select.dataset.filter;
  if (filter && Object.hasOwn(eventFilters, filter)) {
    eventFilters[filter] = select.value;
    renderEvents();
  }
});
