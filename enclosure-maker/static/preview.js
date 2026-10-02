const statusEl = document.getElementById('status');
const paramsEl = document.getElementById('params');
const paramsBodyEl = document.getElementById('params-body');
const paramsToggleEl = document.getElementById('params-toggle');
const chatEl = document.getElementById('chat');
const chatLogEl = document.getElementById('chat-log');
const chatFormEl = document.getElementById('chat-form');
const chatInputEl = document.getElementById('chat-input');
const chatSendEl = document.getElementById('chat-send');
const chatToggleEl = document.getElementById('chat-toggle');
const assistantProviderEl = document.getElementById('assistant-provider');
const saveBtnEl = document.getElementById('save-btn');
const exportBtnEl = document.getElementById('export-btn');
const newChatBtnEl = document.getElementById('new-chat-btn');
const historyBtnEl = document.getElementById('history-btn');
const historyCloseBtnEl = document.getElementById('history-close-btn');
const historyPanelEl = document.getElementById('history-panel');
const historyListEl = document.getElementById('history-list');
const viewportEl = document.getElementById('viewport');
const layoutEl = document.getElementById('layout');
const codePaneEl = document.getElementById('code-pane');
const codeEl = document.getElementById('code');
const codeNameEl = document.getElementById('code-name');
const codeCollapseEl = document.getElementById('code-collapse');
const codeBtnEl = document.getElementById('code-btn');
const previewBtnEl = document.getElementById('preview-btn');
const codeSplitterEl = document.getElementById('code-splitter');

const scene = new THREE.Scene();
scene.background = new THREE.Color(0x1e1f22);

const camera = new THREE.PerspectiveCamera(45, 1, 0.1, 10000);
camera.position.set(80, 80, 80);
camera.up.set(0, 0, 1);

let renderer = null;
try {
  renderer = new THREE.WebGLRenderer({ antialias: false, powerPreference: 'low-power' });
  renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 1.5));
  viewportEl.appendChild(renderer.domElement);
} catch (err) {
  statusEl.textContent = `3D view failed to start: ${err && err.message ? err.message : err}`;
}

function resizeViewport() {
  if (!renderer) return;
  const w = viewportEl.clientWidth;
  const h = viewportEl.clientHeight;
  if (w === 0 || h === 0) return;
  camera.aspect = w / h;
  camera.updateProjectionMatrix();
  renderer.setSize(w, h);
}
resizeViewport();
if (renderer) new ResizeObserver(resizeViewport).observe(viewportEl);

const controls = renderer ? new THREE.OrbitControls(camera, renderer.domElement) : null;
if (controls) controls.enableDamping = true;

scene.add(new THREE.AmbientLight(0xffffff, 0.5));
const key = new THREE.DirectionalLight(0xffffff, 0.8);
key.position.set(1, 2, 1.5);
scene.add(key);
const fill = new THREE.DirectionalLight(0xffffff, 0.3);
fill.position.set(-1, -0.5, -1);
scene.add(fill);

const bedGrid = new THREE.GridHelper(200, 20, 0x3a3d42, 0x2a2c30);
bedGrid.rotateX(Math.PI / 2);
scene.add(bedGrid);
scene.add(new THREE.AxesHelper(20));

const material = new THREE.MeshStandardMaterial({
  color: 0x5b8dee,
  metalness: 0.1,
  roughness: 0.6,
  side: THREE.DoubleSide,
});

let mesh = new THREE.Mesh(new THREE.BufferGeometry(), material);
scene.add(mesh);

function applyMeshBytes(buffer) {
  try {
    const dv = new DataView(buffer);
    const triCount = dv.getUint32(0, true);
    const byteLength = buffer.byteLength || buffer.length || 0;
    if (4 + triCount * 36 > byteLength) {
      statusEl.textContent = 'preview mesh was truncated';
      return;
    }
    const positions = new Float32Array(buffer, 4, triCount * 9);

    const geo = new THREE.BufferGeometry();
    geo.setAttribute('position', new THREE.BufferAttribute(positions, 3));
    geo.computeVertexNormals();
    geo.computeBoundingSphere();

    mesh.geometry.dispose();
    mesh.geometry = geo;
    modelTools.setGeometry(buffer);

    statusEl.textContent = `${triCount} triangles`;
  } catch (err) {
    statusEl.textContent = `preview failed: ${err && err.message ? err.message : err}`;
  }
}

// Debounced per-parameter send: the slider's number updates instantly, but
// the (potentially expensive) server-side re-eval only fires ~80ms after the
// user stops moving that slider.
const paramSendTimers = {};
let socket = null;
const modelTools = new ModelTools({
  scene, camera, renderer, orbit: controls,
  onCommit: (name, transform, requestId) => {
    if (!socket || socket.readyState !== WebSocket.OPEN) throw new Error('Reconnect before changing a part.');
    socket.send(JSON.stringify({ kind: 'part_transform', name, ...transform, request_id: requestId }));
  },
});

function sendParam(name, value) {
  clearTimeout(paramSendTimers[name]);
  paramSendTimers[name] = setTimeout(() => {
    if (socket && socket.readyState === WebSocket.OPEN) {
      socket.send(JSON.stringify({ name, value }));
    }
  }, 80);
}

function formatNumber(n) {
  return Math.round(n * 100) / 100;
}

function applyParams(params) {
  if (!Array.isArray(params) || params.length === 0) {
    paramsEl.hidden = true;
    saveBtnEl.hidden = true;
    return;
  }
  paramsEl.hidden = false;
  saveBtnEl.hidden = false;
  paramsBodyEl.innerHTML = '';
  for (const p of params) {
    const row = document.createElement('div');
    row.className = 'param-row';

    const label = document.createElement('label');
    label.textContent = p.name;

    const valueEl = document.createElement('input');
    valueEl.type = 'number';
    valueEl.min = p.min; valueEl.max = p.max; valueEl.step = 'any';
    valueEl.setAttribute('aria-label', p.name);
    valueEl.className = 'param-value';
    valueEl.value = p.value;

    const slider = document.createElement('input');
    slider.type = 'range';
    slider.min = p.min;
    slider.max = p.max;
    slider.step = (p.max - p.min) / 200 || 0.01;
    slider.value = p.value;
    slider.addEventListener('input', () => {
      const value = parseFloat(slider.value);
      valueEl.value = value;
      sendParam(p.name, value);
    });

    valueEl.addEventListener('change', () => {
      if (!valueEl.checkValidity() || !Number.isFinite(valueEl.valueAsNumber)) { valueEl.value = p.value; return; }
      slider.value = valueEl.value; sendParam(p.name, valueEl.valueAsNumber);
    });
    const reset = document.createElement('button'); reset.textContent = '↺'; reset.title = `Reset ${p.name} to ${p.default}`;
    reset.addEventListener('click', () => { slider.value = p.default; valueEl.value = p.default; sendParam(p.name, p.default); });
    row.appendChild(label);
    row.appendChild(valueEl);
    row.appendChild(reset);
    row.appendChild(slider);
    paramsBodyEl.appendChild(row);
  }
}

// ---------- Assistant chat ----------
//
// One text frame per server-side push, JSON-encoded. A bare array is a
// parameter schema update (see applyParams above); an object with a "kind"
// field is a chat protocol message: {"kind":"chat_event","event":{...}}
// wraps one normalized provider event (Claude's native shape is the
// canonical format; Codex and Copilot JSONL are adapted server-side), and
// {"kind":"chat_error","message":"..."} reports a session-start/send
// failure. {"kind":"script","source":"..."} is the watched file (pushed on
// connect and after a disk change), and {"kind":"script_error","message":"..."}
// is a failed eval that leaves the last good mesh on screen. See
// crates/em-agent's protocol module for the chat event shape.

let currentAssistantEl = null;
let turnInFlight = false;
let thinkingEl = null;
// tool_use.id -> { el, baseText, name } so the matching tool_result (which
// arrives as a separate 'user' event, correlated only by id) updates the
// same line instead of appearing as a disconnected message.
let pendingTools = {};
// content_block index -> pending tool, so input_json_delta events (which
// only carry an index) can update the line started by content_block_start.
let streamingToolByIndex = {};
let streamedThinkingBlocks = 0;
let thinkingBodyEl = null;
let toolCallCount = 0;
let currentProvider = assistantProviderEl.value;

function appendChatMsg(kind, text) {
  const el = document.createElement('div');
  el.className = `chat-msg ${kind}`;
  el.textContent = text;
  chatLogEl.appendChild(el);
  chatLogEl.scrollTop = chatLogEl.scrollHeight;
  return el;
}

function clearThinking() {
  if (thinkingEl) {
    thinkingEl.remove();
    thinkingEl = null;
  }
}

function setTurnInFlight(inFlight) {
  turnInFlight = inFlight;
  chatSendEl.disabled = inFlight;
  if (!inFlight) {
    currentAssistantEl = null;
    pendingTools = {};
    streamingToolByIndex = {};
    streamedThinkingBlocks = 0;
    thinkingBodyEl = null;
    clearThinking();
  } else {
    toolCallCount = 0;
    streamingToolByIndex = {};
    streamedThinkingBlocks = 0;
    thinkingBodyEl = null;
  }
}

// A short, human-readable description of what a tool call is actually
// doing -- not just its name. Glob/Grep in particular carry no file_path,
// so without reading `pattern` explicitly a call shows as a bare "Glob"
// with zero information about what was searched for (the exact gap a real
// transcript surfaced).
function describeToolUse(block) {
  const input = block.input || {};
  switch (block.name) {
    case 'Read':
    case 'Write':
    case 'read':
    case 'write':
      return `${block.name} ${input.file_path || input.path || '?'}`;
    case 'MultiEdit':
      return `MultiEdit ${input.file_path || '?'} (${(input.edits || []).length} edits)`;
    case 'NotebookEdit':
      return `NotebookEdit ${input.notebook_path || '?'}`;
    case 'Shell':
      return `Shell ${input.command || '?'}`;
    case 'Edit':
      if (Array.isArray(input.changes)) {
        return `Edit ${input.changes.map((change) => change.path).filter(Boolean).join(', ') || '?'}`;
      }
      return `Edit ${input.file_path || '?'}`;
    case 'Glob':
      return `Glob "${input.pattern || '?'}"` + (input.path ? ` in ${input.path}` : '');
    case 'Grep':
      return `Grep "${input.pattern || '?'}"` + (input.path ? ` in ${input.path}` : '');
    default: {
      const path = input.file_path || input.notebook_path;
      if (path) return `${block.name} ${path}`;
      const keys = Object.keys(input);
      return keys.length ? `${block.name} ${JSON.stringify(input)}` : block.name;
    }
  }
}

// A short summary of a tool's result -- not the raw content (a Read of a
// long file would otherwise dump its entire text into the chat log), just
// enough to see at a glance whether it did something and roughly what.
function summarizeToolResult(name, content) {
  const text = typeof content === 'string' ? content : Array.isArray(content)
    ? content.map((b) => (b && b.text) || '').join('\n')
    : JSON.stringify(content);
  if (!text) return 'done';
  switch (name) {
    case 'Read': {
      const lines = text.split('\n').length;
      return `${lines} line${lines === 1 ? '' : 's'}`;
    }
    case 'Glob': {
      const count = text.trim() ? text.trim().split('\n').length : 0;
      return `${count} match${count === 1 ? '' : 'es'}`;
    }
    case 'Grep': {
      const count = text.trim() ? text.trim().split('\n').length : 0;
      return `${count} result${count === 1 ? '' : 's'}`;
    }
    default: {
      const trimmed = text.trim();
      if (trimmed.length <= 240) return trimmed;
      return trimmed.slice(0, 240) + '…';
    }
  }
}

function describePartialTool(name, jsonText) {
  try {
    return describeToolUse({ name, input: JSON.parse(jsonText) });
  } catch {
    const path = jsonText.match(/"(?:file_path|notebook_path|path)"\s*:\s*"([^"]*)"/);
    if (path) return `${name} ${path[1]}`;
    const pattern = jsonText.match(/"pattern"\s*:\s*"([^"]*)"/);
    if (pattern) return `${name} "${pattern[1]}"`;
    const command = jsonText.match(/"command"\s*:\s*"([^"]*)"/);
    if (command) return `${name} ${command[1]}`;
    return name || 'tool';
  }
}

function ensureToolLine(block) {
  const id = block.id || '';
  if (id && pendingTools[id]) return pendingTools[id];
  toolCallCount += 1;
  const baseText = describeToolUse(block);
  const el = appendChatMsg('tool', `${baseText} …`);
  const pending = { el, baseText, name: block.name || 'tool', json: '', settled: false };
  if (id) pendingTools[id] = pending;
  clearThinking();
  return pending;
}

function appendThinking(text) {
  if (!text) return;
  clearThinking();
  if (!thinkingBodyEl) {
    thinkingBodyEl = appendChatMsg('thinking', '');
  }
  thinkingBodyEl.textContent += text;
  thinkingBodyEl.scrollTop = thinkingBodyEl.scrollHeight;
  chatLogEl.scrollTop = chatLogEl.scrollHeight;
}

function handleChatEvent(evt) {
  if (!evt || typeof evt !== 'object') return;

  switch (evt.type) {
    case 'system':
      if (evt.subtype === 'init') {
        clearThinking();
        const provider = evt.provider || currentProvider;
        appendChatMsg('system', `${provider} ready (${evt.model || 'unknown model'})`);
      }
      break;

    case 'stream_event': {
      const inner = evt.event || {};
      if (inner.type === 'content_block_start') {
        const block = inner.content_block || {};
        if (block.type === 'tool_use') {
          const pending = ensureToolLine(block);
          if (inner.index != null) streamingToolByIndex[inner.index] = pending;
        } else if (block.type === 'thinking') {
          thinkingBodyEl = null;
          if (block.thinking) {
            streamedThinkingBlocks += 1;
            appendThinking(block.thinking);
          }
        }
      } else if (inner.type === 'content_block_delta' && inner.delta) {
        const delta = inner.delta;
        if (delta.type === 'text_delta' && delta.text) {
          clearThinking();
          if (!currentAssistantEl) {
            currentAssistantEl = appendChatMsg('assistant', '');
          }
          currentAssistantEl.textContent += delta.text;
          chatLogEl.scrollTop = chatLogEl.scrollHeight;
        } else if (delta.type === 'thinking_delta' && delta.thinking) {
          if (!thinkingBodyEl) streamedThinkingBlocks += 1;
          appendThinking(delta.thinking);
        } else if (delta.type === 'input_json_delta' && delta.partial_json) {
          const pending = streamingToolByIndex[inner.index];
          if (pending && !pending.settled) {
            pending.json += delta.partial_json;
            pending.baseText = describePartialTool(pending.name, pending.json);
            pending.el.textContent = `${pending.baseText} …`;
            chatLogEl.scrollTop = chatLogEl.scrollHeight;
          }
        }
      }
      break;
    }

    case 'assistant': {
      const content = (evt.message && evt.message.content) || [];
      const hadStreamedText = currentAssistantEl !== null;
      let thinkingSkip = streamedThinkingBlocks;
      streamedThinkingBlocks = 0;
      thinkingBodyEl = null;
      for (const block of content) {
        if (block.type === 'text' && !hadStreamedText && block.text) {
          clearThinking();
          appendChatMsg('assistant', block.text);
        } else if (block.type === 'thinking' && block.thinking) {
          if (thinkingSkip > 0) {
            thinkingSkip -= 1;
          } else {
            appendThinking(block.thinking);
            thinkingBodyEl = null;
          }
        } else if (block.type === 'tool_use') {
          const pending = block.id && pendingTools[block.id];
          if (pending) {
            pending.name = block.name || pending.name;
            pending.baseText = describeToolUse(block);
            if (!pending.settled) pending.el.textContent = `${pending.baseText} …`;
          } else {
            ensureToolLine(block);
          }
        }
      }
      currentAssistantEl = null;
      break;
    }

    case 'user': {
      const content = (evt.message && evt.message.content) || [];
      for (const block of content) {
        if (block.type !== 'tool_result') continue;
        clearThinking();
        const pending = pendingTools[block.tool_use_id];
        if (block.is_error) {
          const text = typeof block.content === 'string' ? block.content : JSON.stringify(block.content);
          if (pending) {
            pending.settled = true;
            pending.el.className = 'chat-msg error';
            pending.el.textContent = `${pending.baseText} → ✗ ${text}`;
          } else {
            appendChatMsg('error', text);
          }
        } else if (pending) {
          pending.settled = true;
          const summary = summarizeToolResult(pending.name, block.content);
          pending.el.textContent = `${pending.baseText} → ${summary}`;
        }
        if (pending) delete pendingTools[block.tool_use_id];
      }
      if (turnInFlight) {
        clearThinking();
        thinkingEl = appendChatMsg('system', 'working…');
      }
      break;
    }

    case 'result': {
      if (evt.is_error) {
        appendChatMsg('error', evt.result || 'the assistant reported an error');
      } else {
        const parts = [];
        if (toolCallCount > 0) parts.push(`${toolCallCount} tool call${toolCallCount === 1 ? '' : 's'}`);
        if (typeof evt.total_cost_usd === 'number') parts.push(`$${evt.total_cost_usd.toFixed(4)}`);
        appendChatMsg('system', parts.length ? `done · ${parts.join(' · ')}` : 'done');
      }
      setTurnInFlight(false);
      break;
    }
  }
}

// ---------- Chat history ----------
//
// Persisted chats are replayed by feeding their stored ops back through the
// same rendering path live events use (`handleChatEvent` for `event` lines),
// so a replayed transcript looks exactly like it did live -- one rendering
// path, not two kept in sync by hand.

function formatHistoryTime(unixSecs) {
  if (!unixSecs) return '';
  return new Date(unixSecs * 1000).toLocaleString();
}

function replayChatLines(lines) {
  chatLogEl.innerHTML = '';
  currentAssistantEl = null;
  pendingTools = {};
  streamingToolByIndex = {};
  streamedThinkingBlocks = 0;
  thinkingBodyEl = null;
  clearThinking();
  toolCallCount = 0;
  for (const line of lines) {
    if (!line || typeof line !== 'object') continue;
    if (line.op === 'userSent') {
      appendChatMsg('user', line.text || '');
    } else if (line.op === 'event') {
      handleChatEvent(line.event);
    }
  }
}

function closeHistoryPanel() {
  historyPanelEl.hidden = true;
}

function renderHistoryList(chats) {
  historyListEl.innerHTML = '';
  if (!Array.isArray(chats) || chats.length === 0) {
    const empty = document.createElement('div');
    empty.className = 'history-empty';
    empty.textContent = 'No past chats yet.';
    historyListEl.appendChild(empty);
    return;
  }
  for (const chat of chats) {
    const item = document.createElement('div');
    item.className = 'history-item';

    const text = document.createElement('div');
    text.className = 'history-item-text';
    const title = document.createElement('div');
    title.className = 'history-item-title';
    title.textContent = chat.title || '(empty chat)';
    const time = document.createElement('div');
    time.className = 'history-item-time';
    time.textContent = `${chat.provider || 'claude'} · ${formatHistoryTime(chat.modified)}`;
    text.appendChild(title);
    text.appendChild(time);

    const del = document.createElement('button');
    del.className = 'history-item-delete';
    del.title = 'Delete this chat';
    del.textContent = '×';
    del.addEventListener('click', (e) => {
      e.stopPropagation();
      if (!socket || socket.readyState !== WebSocket.OPEN) return;
      socket.send(JSON.stringify({ kind: 'history_delete', file: chat.file }));
    });

    item.addEventListener('click', () => {
      if (!socket || socket.readyState !== WebSocket.OPEN || turnInFlight) return;
      socket.send(JSON.stringify({ kind: 'history_continue', file: chat.file }));
    });

    item.appendChild(text);
    item.appendChild(del);
    historyListEl.appendChild(item);
  }
}

newChatBtnEl.addEventListener('click', () => {
  if (!socket || socket.readyState !== WebSocket.OPEN || turnInFlight) return;
  socket.send(JSON.stringify({ kind: 'history_new' }));
});

assistantProviderEl.addEventListener('change', () => {
  if (turnInFlight) {
    assistantProviderEl.value = currentProvider;
    return;
  }
  currentProvider = assistantProviderEl.value;
  if (socket && socket.readyState === WebSocket.OPEN) {
    socket.send(JSON.stringify({ kind: 'history_new' }));
  }
});

historyBtnEl.addEventListener('click', () => {
  historyPanelEl.hidden = false;
  if (socket && socket.readyState === WebSocket.OPEN) {
    socket.send(JSON.stringify({ kind: 'history_list' }));
  }
});

historyCloseBtnEl.addEventListener('click', closeHistoryPanel);

// Shared by the manual submit handler and the bancada-import seed message
// below, so a seeded first request renders and persists exactly like a
// message the user typed themselves -- one send path, not two.
function sendChatText(text) {
  if (!text || turnInFlight || !socket || socket.readyState !== WebSocket.OPEN) return;
  chatEl.classList.remove('collapsed');
  chatToggleEl.textContent = '−';
  chatToggleEl.title = 'Collapse';
  appendChatMsg('user', text);
  socket.send(JSON.stringify({ kind: 'chat', text, provider: currentProvider }));
  setTurnInFlight(true);
  // Cleared by the next handleChatEvent call, whatever it turns out to be
  // -- there can be a real gap here while a session first starts up.
  thinkingEl = appendChatMsg('system', 'thinking…');
}

chatFormEl.addEventListener('submit', (e) => {
  e.preventDefault();
  const text = chatInputEl.value.trim();
  if (!text || turnInFlight || !socket || socket.readyState !== WebSocket.OPEN) return;
  chatInputEl.value = '';
  sendChatText(text);
});

chatInputEl.addEventListener('keydown', (e) => {
  if (e.key === 'Enter' && !e.shiftKey) {
    e.preventDefault();
    chatFormEl.requestSubmit();
  }
});

chatToggleEl.addEventListener('click', () => {
  chatEl.classList.toggle('collapsed');
  chatToggleEl.textContent = chatEl.classList.contains('collapsed') ? '+' : '−';
});

paramsToggleEl.addEventListener('click', () => {
  paramsEl.classList.toggle('collapsed');
  const collapsed = paramsEl.classList.contains('collapsed');
  paramsToggleEl.textContent = collapsed ? '+' : '−';
  paramsToggleEl.title = collapsed ? 'Expand' : 'Collapse';
});

const originalSaveLabel = saveBtnEl.textContent;
function flashSaveLabel(text, ms) {
  saveBtnEl.textContent = text;
  setTimeout(() => { saveBtnEl.textContent = originalSaveLabel; }, ms);
}

saveBtnEl.addEventListener('click', () => {
  if (!socket || socket.readyState !== WebSocket.OPEN) return;
  saveBtnEl.disabled = true;
  socket.send(JSON.stringify({ kind: 'save' }));
});

function handleSaveResult(msg) {
  saveBtnEl.disabled = false;
  if (!msg.ok) {
    flashSaveLabel('Save failed', 2000);
    appendChatMsg('error', `save failed: ${msg.message}`);
    return;
  }
  if (msg.baked && msg.baked.length > 0) {
    flashSaveLabel('Saved ✓', 1500);
  } else {
    flashSaveLabel('Nothing to save', 1500);
  }
}

// ---------- Export ----------
//
// The server returns one binary STL per printable part. A single part is one
// file; several parts are several files, so a slicer never has to split a
// fused mesh. Preview-only views are already left out of the list.
//
// A plain `<a href="/export.stl" download>` does not reliably work inside the
// desktop app's WebView (WebKitGTK on Linux doesn't honor a same-origin
// `download` navigation the way a browser does), and it cannot save more than
// one file. The click always fetches the list. Each STL is saved via Tauri
// directly or via Bancada's parent-frame bridge when embedded in its iframe.
// A standalone browser downloads each file under the server's filename.
function saveBlob(filename, bytes) {
  const blob = new Blob([bytes]);
  const url = URL.createObjectURL(blob);
  const link = document.createElement('a');
  link.href = url;
  link.download = filename;
  link.click();
  URL.revokeObjectURL(url);
}

// Embedded in bancada's own window (an iframe, not a WebKitGTK window of our
// own): `window.__TAURI__` is injected only into a webview's top frame, so
// it's absent here even though the parent frame has it. Ask the parent to
// call `save_stl_to_downloads` on our behalf instead of falling through to
// `saveBlob` — a `download` navigation doesn't reliably work in WebKitGTK
// either way (see this file above), and a plain browser tab (no parent
// frame with Tauri at all) still falls back to it below.
let stlBridgeRequestId = 0;
function saveViaParentBridge(filename, base64) {
  return new Promise((resolve, reject) => {
    const requestId = ++stlBridgeRequestId;
    const timeout = setTimeout(() => {
      window.removeEventListener('message', onMessage);
      reject(new Error('no response from the parent window'));
    }, 5000);
    function onMessage(event) {
      const data = event.data;
      if (!data || data.type !== 'enclosure-maker:save-stl-result' || data.requestId !== requestId) return;
      clearTimeout(timeout);
      window.removeEventListener('message', onMessage);
      if (data.ok) resolve(data.path);
      else reject(new Error(data.error || 'save failed'));
    }
    window.addEventListener('message', onMessage);
    window.parent.postMessage(
      { type: 'enclosure-maker:save-stl', requestId, filename, contentsB64: base64 },
      '*',
    );
  });
}

const originalExportLabel = exportBtnEl.textContent;
let exportBusy = false;
exportBtnEl.addEventListener('click', async (e) => {
  e.preventDefault();
  if (exportBusy) return;
  exportBusy = true;
  try {
    const res = await fetch('/export.stl');
    if (!res.ok) throw new Error(`export failed: ${res.status}`);
    const body = await res.json();
    const files = Array.isArray(body.files) ? body.files : [];
    if (files.length === 0) throw new Error('no model to export yet');
    for (const file of files) {
      if (window.__TAURI__) {
        await window.__TAURI__.core.invoke('save_stl_to_downloads', {
          filename: file.filename,
          contentsB64: file.data,
        });
      } else if (window.self !== window.top) {
        await saveViaParentBridge(file.filename, file.data);
      } else {
        const binary = atob(file.data);
        const bytes = new Uint8Array(binary.length);
        for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
        saveBlob(file.filename, bytes);
      }
    }
    exportBtnEl.textContent = 'Exported';
    setTimeout(() => { exportBtnEl.textContent = originalExportLabel; }, 1500);
  } catch (err) {
    appendChatMsg('error', `export failed: ${err}`);
  } finally {
    exportBusy = false;
  }
});

// ---------- Script editor ----------
//
// Keystrokes are written back to the watched file after a short pause. The
// file watcher reloads the model and echoes the file text; that echo is
// ignored while a local edit is still waiting to be sent, and a matching
// echo never rewrites the textarea (which would jump the cursor).

let scriptSendTimer = null;

function applyRemoteScript(msg) {
  if (msg.name) codeNameEl.textContent = msg.name;
  if (typeof msg.source !== 'string') return;
  if (scriptSendTimer !== null) return;
  applyCodeControls(msg);
  if (codeEl.value === msg.source) return;
  codeEl.value = msg.source;
}

function applyCodeControls(message) {
  const panel = document.getElementById('code-controls');
  panel.replaceChildren();
  const groups = new Map();
  for (const control of message.controls || []) {
    if (!groups.has(control.group)) {
      const section = document.createElement('details'); section.open = control.group === 'Hardware';
      const summary = document.createElement('summary'); summary.textContent = control.group;
      section.appendChild(summary); panel.appendChild(section); groups.set(control.group, section);
    }
    const row = document.createElement('label'); row.className = 'code-control';
    const label = document.createElement('span'); label.textContent = control.label;
    const input = document.createElement('input'); input.type = 'number'; input.value = control.value;
    input.step = control.integer ? '1' : 'any'; input.min = control.min; input.max = control.max;
    input.setAttribute('aria-label', control.label);
    input.addEventListener('change', () => {
      if (!input.checkValidity() || !Number.isFinite(input.valueAsNumber)) { input.value = control.value; return; }
      if (!socket || socket.readyState !== WebSocket.OPEN) return;
      if (codeEl.value !== message.source) { statusEl.textContent = 'Save the pending code edit before changing this dimension.'; return; }
      statusEl.textContent = 'Building model…'; input.disabled = true;
      socket.send(JSON.stringify({ kind: 'code_control', source: message.source, id: control.id, value: input.valueAsNumber }));
    });
    row.append(label, input); groups.get(control.group).appendChild(row);
  }
  if (!groups.size) panel.textContent = 'Dimensions declared with param() appear above. More controls appear for numeric variables and shape or hardware dimensions.';
}

codeEl.addEventListener('input', () => {
  clearTimeout(scriptSendTimer);
  scriptSendTimer = setTimeout(() => {
    scriptSendTimer = null;
    if (socket && socket.readyState === WebSocket.OPEN) {
      statusEl.textContent = 'Building model…';
      socket.send(JSON.stringify({ kind: 'script', source: codeEl.value }));
    }
  }, 250);
});

codeEl.addEventListener('keydown', (e) => {
  if (e.key !== 'Tab') return;
  e.preventDefault();
  const start = codeEl.selectionStart;
  const end = codeEl.selectionEnd;
  codeEl.value = codeEl.value.slice(0, start) + '  ' + codeEl.value.slice(end);
  codeEl.selectionStart = codeEl.selectionEnd = start + 2;
  codeEl.dispatchEvent(new Event('input'));
});

function setCodeCollapsed(collapsed) {
  layoutEl.classList.toggle('code-collapsed', collapsed);
  codeBtnEl.hidden = !collapsed;
  previewBtnEl.hidden = collapsed;
}

codeCollapseEl.addEventListener('click', () => setCodeCollapsed(true));
codeBtnEl.addEventListener('click', () => setCodeCollapsed(false));
previewBtnEl.addEventListener('click', () => {
  setCodeCollapsed(true);
  if (!socket || socket.readyState !== WebSocket.OPEN) return;
  clearTimeout(scriptSendTimer);
  scriptSendTimer = null;
  statusEl.textContent = 'rendering…';
  socket.send(JSON.stringify({ kind: 'preview', source: codeEl.value }));
});

codeSplitterEl.addEventListener('mousedown', (e) => {
  if (layoutEl.classList.contains('code-collapsed')) return;
  e.preventDefault();
  codeSplitterEl.classList.add('dragging');
  const onMove = (ev) => {
    const rect = layoutEl.getBoundingClientRect();
    const splitter = codeSplitterEl.offsetWidth || 5;
    const max = Math.max(0, rect.width - splitter - 160);
    const min = Math.min(200, max);
    const width = Math.max(min, Math.min(max, rect.right - ev.clientX));
    codePaneEl.style.width = width + 'px';
  };
  const onUp = () => {
    codeSplitterEl.classList.remove('dragging');
    window.removeEventListener('mousemove', onMove);
    window.removeEventListener('mouseup', onUp);
  };
  window.addEventListener('mousemove', onMove);
  window.addEventListener('mouseup', onUp);
});

// A bancada import opens this window with `?seed=<text>` -- sent as the
// first chat message once the socket is up, exactly like the user had
// typed and submitted it. Read once: stripped from the URL immediately so
// a later reconnect (or a page refresh) never resends it.
let pendingSeed = new URLSearchParams(location.search).get('seed');
if (pendingSeed) {
  const url = new URL(location.href);
  url.searchParams.delete('seed');
  history.replaceState(null, '', url);
}

function connect() {
  const proto = location.protocol === 'https:' ? 'wss' : 'ws';
  socket = new WebSocket(`${proto}://${location.host}/ws`);
  socket.binaryType = 'arraybuffer';

  socket.onopen = () => {
    previewBtnEl.disabled = false;
    statusEl.textContent = 'connected';
    if (pendingSeed) {
      sendChatText(pendingSeed);
      pendingSeed = null;
    }
  };
  socket.onmessage = (event) => {
    if (typeof event.data !== 'string') {
      applyMeshBytes(event.data);
      return;
    }
    let parsed;
    try {
      parsed = JSON.parse(event.data);
    } catch (e) {
      return;
    }
    if (Array.isArray(parsed)) {
      applyParams(parsed);
    } else if (parsed && parsed.kind === 'parts') {
      modelTools.setParts(parsed.parts);
      mesh.visible = parsed.parts.length === 0;
    } else if (parsed && parsed.kind === 'part_transform_result') {
      modelTools.result(parsed);
    } else if (parsed && parsed.kind === 'code_control_result') {
      if (!parsed.ok) statusEl.textContent = parsed.message || 'Could not update the dimension.';
    } else if (parsed && parsed.kind === 'chat_event') {
      handleChatEvent(parsed.event);
    } else if (parsed && parsed.kind === 'chat_error') {
      appendChatMsg('error', parsed.message);
      setTurnInFlight(false);
    } else if (parsed && parsed.kind === 'save_result') {
      handleSaveResult(parsed);
    } else if (parsed && parsed.kind === 'history_list_result') {
      renderHistoryList(parsed.chats);
    } else if (parsed && parsed.kind === 'history_load_result') {
      if (parsed.ok) replayChatLines(parsed.lines);
    } else if (parsed && parsed.kind === 'history_continue_result') {
      if (parsed.provider) {
        currentProvider = parsed.provider;
        assistantProviderEl.value = parsed.provider;
      }
      replayChatLines(parsed.lines);
      appendChatMsg('system', 'continuing this chat…');
      closeHistoryPanel();
    } else if (parsed && parsed.kind === 'history_new_result') {
      chatLogEl.innerHTML = '';
      currentAssistantEl = null;
      pendingTools = {};
      clearThinking();
      closeHistoryPanel();
    } else if (parsed && parsed.kind === 'history_delete_result') {
      // The server follows this with a fresh history_list_result; nothing
      // to do here beyond letting that repaint the list.
    } else if (parsed && parsed.kind === 'script') {
      applyRemoteScript(parsed);
    } else if (parsed && parsed.kind === 'script_evaluating') {
      statusEl.textContent = 'Building model…';
    } else if (parsed && parsed.kind === 'script_error') {
      statusEl.textContent = parsed.message || 'script error';
    }
  };
  socket.onclose = () => {
    modelTools.disconnect();
    previewBtnEl.disabled = true;
    statusEl.textContent = 'disconnected — retrying…';
    setTimeout(connect, 1000);
  };
  socket.onerror = () => socket.close();
}
connect();

function animate() {
  requestAnimationFrame(animate);
  if (!renderer) return;
  if (controls) controls.update();
  renderer.render(scene, camera);
}
animate();
