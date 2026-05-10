let state = { project: {}, roles: {}, events: [], messages: [], artefacts: [], memories: [], notifications: [], dag_steps: [], active_artefact_id: null, active_step_id: null };
let selectedArtefactId = null;
let selectedStepId = null;
let activeView = 'chat';

async function loadState() {
  const response = await fetch('/api/state');
  state = await response.json();
  render();
}

function connectEvents() {
  const source = new EventSource('/events');
  source.onmessage = (message) => {
    const update = JSON.parse(message.data);
    if (update.type === 'State') state = update.payload;
    if (update.type === 'Event') state.events.push(update.payload);
    if (update.type === 'Notice') console.warn(update.payload);
    if (state.events.length > 200) state.events = state.events.slice(-200);
    loadState();
  };
}

function render() {
  renderHeader();
  renderConversation();
  renderDag();
  renderStepDetail();
  renderNotifications();
}

function renderHeader() {
  const project = state.project || {};
  document.getElementById('project-chip').textContent = project.name || 'SELIUM';
  document.getElementById('chat-view-button').classList.toggle('active', activeView === 'chat');
  document.getElementById('dag-view-button').classList.toggle('active', activeView === 'dag');
  document.getElementById('chat-view').classList.toggle('active', activeView === 'chat');
  document.getElementById('dag-view').classList.toggle('active', activeView === 'dag');
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

function renderDag() {
  const root = document.getElementById('dag');
  const steps = state.dag_steps || [];
  const activeId = selectedStepId || state.active_step_id || (steps[0] && steps[0].id);
  root.innerHTML = steps.length ? '' : '<div class="empty">No project flow yet.</div>';
  for (const step of steps) {
    const button = document.createElement('button');
    button.type = 'button';
    button.className = `dag-step ${step.id === activeId ? 'active' : ''}`;
    button.onclick = () => { selectedStepId = step.id; renderDag(); renderStepDetail(); };
    button.innerHTML = `<strong>${escapeHtml(step.label)}</strong><div class="state">${escapeHtml(step.state)} · ${escapeHtml(step.role)}</div><div>${escapeHtml(step.summary)}</div>`;
    root.appendChild(button);
  }
}

function renderStepDetail() {
  const root = document.getElementById('step-detail');
  const steps = state.dag_steps || [];
  const step = steps.find(s => s.id === (selectedStepId || state.active_step_id)) || steps[0];
  if (!step) {
    root.innerHTML = '<div class="empty">Select a step to inspect artefacts, logs and semantic evidence.</div>';
    return;
  }
  const artefacts = (state.artefacts || []).filter(a => (step.artefact_ids || []).includes(a.id));
  const events = (state.events || []).filter(e => (step.event_ids || []).includes(e.id));
  root.innerHTML = `
    <div class="detail-card"><h3>${escapeHtml(step.label)}</h3><div>${escapeHtml(step.summary)}</div><div class="empty">Role: ${escapeHtml(step.role)} · State: ${escapeHtml(step.state)}</div></div>
    <div class="detail-grid">
      <div class="detail-card"><h3>Artefacts</h3>${artefactsHtml(artefacts)}</div>
      <div class="detail-card"><h3>Logs</h3>${eventsHtml(events)}</div>
      <div class="detail-card"><h3>Memory</h3>${memoriesHtml(state.memories || [])}</div>
      <div class="detail-card"><h3>CoT</h3><p class="empty">Raw chain-of-thought is intentionally not shown. MMAT exposes consequential semantic events, claims, artefacts and evidence instead.</p></div>
    </div>
  `;
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
  return escapeHtml(value)
    .replace(/(@[a-zA-Z][a-zA-Z0-9_-]*)/g, '<span class="mention">$1</span>')
    .replace(/(`[^`]+`)/g, '<span class="code-token">$1</span>');
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

document.getElementById('notification-count').addEventListener('click', (event) => {
  event.stopPropagation();
  document.getElementById('notification-panel').classList.toggle('open');
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
  return artefacts.map(artefact => `<div class="memory"><div class="meta">${escapeHtml(artefact.title)} · ${escapeHtml(artefact.producer_role)}</div><pre>${escapeHtml(JSON.stringify(artefact.content, null, 2))}</pre></div>`).join('');
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
