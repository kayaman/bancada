const statusEl = document.getElementById('status');
const chatLogEl = document.getElementById('chat-log');
const chatFormEl = document.getElementById('chat-form');
const chatInputEl = document.getElementById('chat-input');
const chatSendEl = document.getElementById('chat-send');
const assistantProviderEl = document.getElementById('assistant-provider');
const newChatBtnEl = document.getElementById('new-chat-btn');
const historyBtnEl = document.getElementById('history-btn');
const historyCloseBtnEl = document.getElementById('history-close-btn');
const historyPanelEl = document.getElementById('history-panel');
const historyListEl = document.getElementById('history-list');

let socket = null;

// ---------- Assistant chat ----------
//
// One text frame per server-side push, JSON-encoded, always an object with
// a "kind" field: {"kind":"chat_event","event":{...}} wraps one normalized
// provider event (Claude's native shape is the canonical format; Codex and
// Copilot JSONL are adapted server-side), {"kind":"chat_error","message":"..."}
// reports a session-start/send failure. See crates/em-agent's protocol
// module for the chat event shape. FreeCAD itself -- driven by the
// assistant through its own MCP server -- is the live model; this page
// shows only the conversation, not a 3D view.

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
// doing -- not just its name. The FreeCAD MCP tools (mcp__freecad__*) carry
// their own argument shapes; a few of the most common ones get a tailored
// one-liner, everything else falls through to the generic name+JSON case.
function describeToolUse(block) {
  const input = block.input || {};
  switch (block.name) {
    case 'Read':
    case 'read':
      return `${block.name} ${input.file_path || input.path || '?'}`;
    case 'Glob':
      return `Glob "${input.pattern || '?'}"` + (input.path ? ` in ${input.path}` : '');
    case 'Grep':
      return `Grep "${input.pattern || '?'}"` + (input.path ? ` in ${input.path}` : '');
    case 'mcp__freecad__execute_code':
    case 'mcp__freecad__execute_code_async':
    case 'mcp__freecad__execute_code_headless': {
      const code = (input.code || '').trim();
      const firstLine = code.split('\n')[0] || '?';
      return `FreeCAD: ${firstLine}${code.includes('\n') ? ' …' : ''}`;
    }
    case 'mcp__freecad__create_document':
      return `FreeCAD: new document ${input.name || '?'}`;
    case 'mcp__freecad__create_object':
      return `FreeCAD: create ${input.obj_type || '?'} "${input.obj_name || '?'}"`;
    case 'mcp__freecad__edit_object':
      return `FreeCAD: edit "${input.obj_name || '?'}"`;
    case 'mcp__freecad__delete_object':
      return `FreeCAD: delete "${input.obj_name || '?'}"`;
    case 'mcp__freecad__get_view':
      return 'FreeCAD: view screenshot';
    case 'mcp__freecad__get_rpc_status':
      return 'FreeCAD: check connection';
    case 'mcp__freecad__list_documents':
    case 'mcp__freecad__get_objects':
    case 'mcp__freecad__get_object':
    case 'mcp__freecad__reload_document':
    case 'mcp__freecad__get_parts_list':
    case 'mcp__freecad__insert_part_from_library':
    case 'mcp__freecad__get_async_status':
      return block.name.replace('mcp__freecad__', 'FreeCAD: ');
    default: {
      const path = input.file_path || input.notebook_path;
      if (path) return `${block.name} ${path}`;
      const keys = Object.keys(input);
      return keys.length ? `${block.name} ${JSON.stringify(input)}` : block.name;
    }
  }
}

// A short summary of a tool's result -- not the raw content (a long result
// would otherwise dump its entire text into the chat log), just enough to
// see at a glance whether it did something and roughly what.
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

  socket.onopen = () => {
    statusEl.textContent = 'connected — FreeCAD holds the live model in its own window';
    if (pendingSeed) {
      sendChatText(pendingSeed);
      pendingSeed = null;
    }
  };
  socket.onmessage = (event) => {
    if (typeof event.data !== 'string') return;
    let parsed;
    try {
      parsed = JSON.parse(event.data);
    } catch (e) {
      return;
    }
    if (!parsed || typeof parsed !== 'object') return;
    if (parsed.kind === 'chat_event') {
      handleChatEvent(parsed.event);
    } else if (parsed.kind === 'chat_error') {
      appendChatMsg('error', parsed.message);
      setTurnInFlight(false);
    } else if (parsed.kind === 'history_list_result') {
      renderHistoryList(parsed.chats);
    } else if (parsed.kind === 'history_load_result') {
      if (parsed.ok) replayChatLines(parsed.lines);
    } else if (parsed.kind === 'history_continue_result') {
      if (parsed.provider) {
        currentProvider = parsed.provider;
        assistantProviderEl.value = parsed.provider;
      }
      replayChatLines(parsed.lines);
      appendChatMsg('system', 'continuing this chat…');
      closeHistoryPanel();
    } else if (parsed.kind === 'history_new_result') {
      chatLogEl.innerHTML = '';
      currentAssistantEl = null;
      pendingTools = {};
      clearThinking();
      closeHistoryPanel();
    } else if (parsed.kind === 'history_delete_result') {
      // The server follows this with a fresh history_list_result; nothing
      // to do here beyond letting that repaint the list.
    }
  };
  socket.onclose = () => {
    statusEl.textContent = 'disconnected — retrying…';
    setTimeout(connect, 1000);
  };
  socket.onerror = () => socket.close();
}
connect();
