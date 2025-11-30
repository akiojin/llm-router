(() => {
  const fallbackModels = [
    "gpt-oss:20b",
    "gpt-oss:7b",
    "gpt-oss:3b",
    "gpt-oss:1b",
    "gpt-oss-safeguard:20b",
    "qwen3-coder:30b",
  ];

  const CLOUD_PREFIXES = [
    "openai:",
    "azure:",
    "anthropic:",
    "google:",
    "vertex:",
    "gcp:",
    "aws:",
    "bedrock:",
    "cohere:",
  ];

  const STORAGE_KEY = "router:chat:sessions:v1";

  const dom = {
    modelSelect: document.getElementById("model-select"),
    systemPrompt: document.getElementById("system-prompt"),
    appendSystem: document.getElementById("append-system"),
    streamToggle: document.getElementById("stream-toggle"),
    chatLog: document.getElementById("chat-log"),
    chatForm: document.getElementById("chat-form"),
    chatInput: document.getElementById("chat-input"),
    sendButton: document.getElementById("send-button"),
    stopButton: document.getElementById("stop-button"),
    resetButton: document.getElementById("reset-chat"),
    errorBanner: document.getElementById("error-banner"),
    routerStatus: document.getElementById("router-status"),
    modelCount: document.getElementById("model-count"),
    modelHint: document.getElementById("model-hint"),
    copyCurl: document.getElementById("copy-curl"),
    messageTemplate: document.getElementById("message-template"),
    sessionList: document.getElementById("session-list"),
    newChat: document.getElementById("new-chat"),
    newChatInline: document.getElementById("new-chat-inline"),
    providerToggle: document.getElementById("provider-toggle"),
    activeSessionTitle: document.getElementById("active-session-title"),
    activeSessionMeta: document.getElementById("active-session-meta"),
    chatMeta: document.getElementById("chat-meta-hint"),
  };
  dom.providerButtons = dom.providerToggle
    ? Array.from(dom.providerToggle.querySelectorAll(".segmented__btn"))
    : [];

  const state = {
    sessions: [],
    activeSessionId: null,
    history: [],
    models: [],
    providerFilter: "local",
    loading: false,
    controller: null,
    endpoint: "/api/chat",
    pendingAssistant: null,
  };

  const timeFormatter = new Intl.DateTimeFormat("ja-JP", {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  });

  function formatTime(value) {
    if (!value) return "";
    const date = value instanceof Date ? value : new Date(value);
    return timeFormatter.format(date);
  }

  function setStatus(text, variant = "online") {
    const el = dom.routerStatus;
    if (!el) return;
    el.textContent = text;

    el.classList.remove(
      "status-pill--connecting",
      "status-pill--offline",
      "status-pill--online",
      "status-pill--error",
    );

    const map = {
      online: "status-pill--online",
      offline: "status-pill--offline",
      connecting: "status-pill--connecting",
      error: "status-pill--error",
    };

    el.classList.add(map[variant] ?? "status-pill--online");
  }

  function showError(message) {
    if (!dom.errorBanner) return;
    dom.errorBanner.textContent = message;
    dom.errorBanner.classList.remove("hidden");
  }

  function clearError() {
    dom.errorBanner?.classList.add("hidden");
    if (dom.errorBanner) dom.errorBanner.textContent = "";
  }

  function modelKind(id) {
    const lower = (id || "").toLowerCase();
    return CLOUD_PREFIXES.some((prefix) => lower.startsWith(prefix)) ? "cloud" : "local";
  }

  function providerName(id) {
    const lower = (id || "").toLowerCase();
    const hit = CLOUD_PREFIXES.find((prefix) => lower.startsWith(prefix));
    if (hit) return hit.replace(/:$/, "");
    return "local";
  }

  function filteredModelIds() {
    const filtered = state.models.filter((id) =>
      state.providerFilter === "all" ? true : modelKind(id) === state.providerFilter,
    );
    return filtered;
  }

  function updateModelHint(displayed, fallbackUsed) {
    const counts = state.models.reduce(
      (acc, id) => {
        acc[modelKind(id)] += 1;
        return acc;
      },
      { local: 0, cloud: 0 },
    );

    if (dom.modelCount) {
      dom.modelCount.textContent = `ローカル ${counts.local} / クラウド ${counts.cloud}`;
    }

    if (dom.modelHint) {
      const filterLabel =
        state.providerFilter === "all"
          ? "フィルター: すべて"
          : state.providerFilter === "local"
            ? "フィルター: ローカル"
            : "フィルター: クラウド";
      const displayCount = displayed.length;
      const tail = fallbackUsed
        ? "該当モデルがないため全モデルを表示中"
        : `表示中: ${displayCount}件`;
      dom.modelHint.textContent = `${filterLabel} · ${tail}`;
      if (dom.chatMeta) {
        const chosen = dom.modelSelect?.value || "-";
        const scopeLabel = state.providerFilter === "all" ? "全モデル" : filterLabel.replace("フィルター: ", "");
        dom.chatMeta.textContent = `${scopeLabel} · 選択モデル: ${chosen}`;
      }
    }
  }

  function renderModels(preferredModel) {
    if (!dom.modelSelect) return;

    const matches = filteredModelIds();
    const fallbackUsed = state.providerFilter !== "all" && matches.length === 0;
    const models =
      matches.length > 0 ? matches : state.models.length > 0 ? state.models : [...fallbackModels];

    dom.modelSelect.innerHTML = "";

    models.forEach((model, index) => {
      const option = document.createElement("option");
      option.value = model;
      option.textContent = model;
      if (preferredModel ? model === preferredModel : index === 0) {
        option.selected = true;
      }
      dom.modelSelect.appendChild(option);
    });

    if (preferredModel && !models.includes(preferredModel) && dom.modelSelect.options.length > 0) {
      dom.modelSelect.value = dom.modelSelect.options[0].value;
    }

    dom.modelSelect.disabled = models.length === 0;
    updateModelHint(models, fallbackUsed);
  }

  async function loadModels() {
    setStatus("Fetching models...", "connecting");
    try {
      const res = await fetch("/v1/models");
      if (!res.ok) {
        throw new Error(`HTTP ${res.status}`);
      }
      const body = await res.json();
      const models = (body?.data || []).map((item) => item.id).filter(Boolean);
      if (!models.length) {
        throw new Error("Model list is empty");
      }
      state.models = models;
      renderModels(currentSession()?.modelId);
      setStatus("Connected to router", "online");
    } catch (err) {
      state.models = [...fallbackModels];
      renderModels(currentSession()?.modelId);
      setStatus("Failed to fetch model list", "error");
      showError(`モデル一覧の取得に失敗しました: ${err.message ?? err}`);
    }
  }

  function touchSession(session) {
    if (!session) return;
    session.updatedAt = new Date().toISOString();
    state.sessions.sort(
      (a, b) => new Date(b.updatedAt || b.createdAt) - new Date(a.updatedAt || a.createdAt),
    );
  }

  function persistSessions() {
    try {
      const payload = state.sessions.map((session) => ({
        id: session.id,
        title: session.title,
        modelId: session.modelId,
        modelScope: session.modelScope,
        createdAt: session.createdAt,
        updatedAt: session.updatedAt,
        history: (session.history || []).map((msg) => ({
          role: msg.role,
          content: msg.content,
          model: msg.model,
          createdAt: msg.createdAt instanceof Date ? msg.createdAt.toISOString() : msg.createdAt,
        })),
      }));
      localStorage.setItem(STORAGE_KEY, JSON.stringify(payload));
    } catch (_err) {
      // 永続化失敗は致命的ではないため無視
    }
    renderSessionList();
  }

  function hydrateSessions() {
    try {
      const raw = localStorage.getItem(STORAGE_KEY);
      if (!raw) return;
      const parsed = JSON.parse(raw);
      state.sessions = (parsed || []).map((session) => ({
        id: session.id,
        title: session.title || "新規チャット",
        modelId: session.modelId,
        modelScope: session.modelScope || "local",
        createdAt: session.createdAt || new Date().toISOString(),
        updatedAt: session.updatedAt || session.createdAt || new Date().toISOString(),
        history: (session.history || []).map((msg) => ({
          role: msg.role,
          content: msg.content,
          model: msg.model,
          createdAt: msg.createdAt ? new Date(msg.createdAt) : new Date(),
          element: null,
        })),
      }));
    } catch (_err) {
      state.sessions = [];
    }
  }

  function ensureActiveSession() {
    if (!state.sessions.length) {
      createSession({ persist: false });
    }
    if (!state.activeSessionId && state.sessions.length) {
      state.activeSessionId = state.sessions[0].id;
    }
    state.history = currentSession()?.history ?? [];
  }

  function currentSession() {
    return state.sessions.find((session) => session.id === state.activeSessionId) || null;
  }

  function preferredModelId() {
    const candidates = filteredModelIds();
    if (candidates.length) return candidates[0];
    if (state.models.length) return state.models[0];
    return fallbackModels[0];
  }

  function updateSessionHeader(session) {
    if (!session) return;
    if (dom.activeSessionTitle) dom.activeSessionTitle.textContent = session.title || "新規チャット";
    if (dom.activeSessionMeta) {
      const messageCount = session.history?.length ?? 0;
      const updated = formatTime(session.updatedAt);
      const modelLabel = session.modelId || preferredModelId();
      dom.activeSessionMeta.textContent = `${session.modelScope || "local"} · ${modelLabel} · メッセージ ${messageCount}件 · 更新 ${updated || "-"}`;
    }
  }

  function renderSessionList() {
    if (!dom.sessionList) return;
    dom.sessionList.innerHTML = "";

    if (!state.sessions.length) {
      const empty = document.createElement("li");
      empty.className = "session-empty";
      empty.textContent = "まだセッションがありません。新規チャットを作成してください。";
      dom.sessionList.appendChild(empty);
      return;
    }

    for (const session of state.sessions) {
      const item = document.createElement("li");
      const isActive = session.id === state.activeSessionId;
      item.className = `session-item${isActive ? " session-item--active" : ""}`;
      item.dataset.sessionId = session.id;

      const title = document.createElement("p");
      title.className = "session-title";
      title.textContent = session.title || "新規チャット";

      const meta = document.createElement("p");
      meta.className = "session-meta";
      const count = session.history?.length ?? 0;
      meta.textContent = `${count}件 · 更新 ${formatTime(session.updatedAt) || "-"}`;

      const badges = document.createElement("div");
      badges.className = "session-badges";

      const provider = document.createElement("span");
      const kind = session.modelScope || "local";
      provider.className = `session-pill ${kind === "cloud" ? "session-pill--cloud" : ""}`;
      provider.textContent = kind === "cloud" ? "Cloud" : "Local";
      badges.appendChild(provider);

      if (session.modelId) {
        const model = document.createElement("span");
        model.className = "session-pill";
        model.textContent = session.modelId;
        badges.appendChild(model);
      }

      item.appendChild(title);
      item.appendChild(badges);
      item.appendChild(meta);

      dom.sessionList.appendChild(item);
    }
  }

  function renderHistory(history) {
    if (!dom.chatLog) return;
    dom.chatLog.innerHTML = "";
    if (!history || !history.length) {
      dom.chatLog.innerHTML =
        '<div class="chat-empty">No messages yet. Send a message using the input below.</div>';
      return;
    }
    for (const entry of history) {
      entry.element = null;
      renderMessage(entry);
    }
  }

  function setActiveSession(sessionId) {
    const session = state.sessions.find((item) => item.id === sessionId);
    if (!session) return;
    state.activeSessionId = sessionId;
    state.history = session.history;
    state.providerFilter = session.modelScope || state.providerFilter;
    renderProviderButtons();
    renderModels(session.modelId);
    renderHistory(session.history);
    updateSessionHeader(session);
    clearError();
    persistSessions();
  }

  function createSession({ title = "新規チャット", persist = true } = {}) {
    const id =
      crypto.randomUUID?.() || `session-${Date.now()}-${Math.random().toString(16).slice(2, 8)}`;
    const now = new Date().toISOString();
    const session = {
      id,
      title,
      modelId: preferredModelId(),
      modelScope: state.providerFilter,
      createdAt: now,
      updatedAt: now,
      history: [],
    };
    state.sessions.unshift(session);
    state.activeSessionId = session.id;
    state.history = session.history;
    touchSession(session);
    if (persist) persistSessions();
    renderSessionList();
    renderHistory(session.history);
    renderProviderButtons();
    renderModels(session.modelId);
    updateSessionHeader(session);
    dom.chatInput?.focus();
  }

  function messageLabel(role) {
    switch (role) {
      case "assistant":
        return "Assistant";
      case "system":
        return "System";
      default:
        return "User";
    }
  }

  function messageMeta(entry) {
    const time = formatTime(entry.createdAt);
    if (entry.role === "assistant") {
      const model = entry.model || dom.modelSelect?.value || "model";
      return `${model} · ${time}`;
    }
    return time;
  }

  function renderMessage(entry) {
    const template = dom.messageTemplate;
    if (!template || !dom.chatLog) return;
    const node = template.content.firstElementChild.cloneNode(true);
    node.dataset.messageId = entry.id;
    node.classList.add(`message--${entry.role}`);

    const roleEl = node.querySelector(".message__role");
    const textEl = node.querySelector(".message__text");
    const metaEl = node.querySelector(".message__meta");

    roleEl.textContent = messageLabel(entry.role);
    textEl.textContent = entry.content;
    metaEl.textContent = messageMeta(entry);

    dom.chatLog.querySelector(".chat-empty")?.remove();
    dom.chatLog.appendChild(node);
    dom.chatLog.scrollTop = dom.chatLog.scrollHeight;
    entry.element = node;
  }

  function updateMessage(entry, content) {
    entry.content = content;
    if (entry.element) {
      const textEl = entry.element.querySelector(".message__text");
      const metaEl = entry.element.querySelector(".message__meta");
      if (textEl) textEl.textContent = content;
      if (metaEl) metaEl.textContent = messageMeta(entry);
    }
    persistSessions();
  }

  function addMessage(role, content, { model } = {}) {
    const entry = {
      id: crypto.randomUUID ? crypto.randomUUID() : `msg-${Date.now()}-${Math.random()}`,
      role,
      content,
      model: model || null,
      createdAt: new Date(),
      element: null,
    };
    state.history.push(entry);
    renderMessage(entry);
    const session = currentSession();
    touchSession(session);
    if (session) {
      session.history = state.history;
      session.modelId = session.modelId || dom.modelSelect?.value || preferredModelId();
    }
    persistSessions();
    updateSessionHeader(session);
    return entry;
  }

  function updateSessionTitleFrom(entry) {
    const session = currentSession();
    if (!session || entry.role !== "user") return;
    const defaultTitle = "新規チャット";
    const shouldUpdate = session.title === defaultTitle || session.history.length === 1;
    if (!shouldUpdate) return;
    const preview = entry.content.slice(0, 32).replace(/\s+/g, " ");
    session.title = preview || defaultTitle;
    touchSession(session);
    persistSessions();
    updateSessionHeader(session);
  }

  function clearHistory() {
    const session = currentSession();
    if (!session) return;
    session.history = [];
    state.history = session.history;
    renderHistory(session.history);
    updateSessionHeader(session);
    persistSessions();
  }

  function buildMessagesForRequest() {
    const messages = [];
    const system = dom.systemPrompt?.value.trim();
    if (dom.appendSystem?.checked && system) {
      messages.push({ role: "system", content: system });
    }
    for (const msg of state.history) {
      messages.push({ role: msg.role, content: msg.content });
    }
    return messages;
  }

  function selectedModel() {
    const current = dom.modelSelect?.value?.trim();
    const model = current || preferredModelId();
    const session = currentSession();
    if (session) {
      session.modelId = model;
      session.modelScope = state.providerFilter;
      touchSession(session);
      persistSessions();
      updateSessionHeader(session);
    }
    return model;
  }

  function setLoading(isLoading, { streaming = false } = {}) {
    state.loading = isLoading;
    dom.sendButton.disabled = isLoading;
    dom.stopButton.disabled = !isLoading;
    dom.streamToggle.disabled = isLoading;
    if (dom.modelSelect) dom.modelSelect.disabled = isLoading;
    if (dom.resetButton) dom.resetButton.disabled = isLoading;
    if (dom.copyCurl) dom.copyCurl.disabled = isLoading;
    if (dom.appendSystem) dom.appendSystem.disabled = isLoading;
    if (dom.systemPrompt) dom.systemPrompt.readOnly = isLoading;
    dom.providerButtons?.forEach((btn) => (btn.disabled = isLoading));
    if (dom.newChat) dom.newChat.disabled = isLoading;
    if (dom.newChatInline) dom.newChatInline.disabled = isLoading;
    if (!streaming) {
      dom.chatInput.readOnly = isLoading;
    } else {
      dom.chatInput.readOnly = false;
    }
  }

  function safeParse(line) {
    try {
      return JSON.parse(line);
    } catch (_err) {
      return null;
    }
  }

  async function postChat(payload, signal) {
    const res = await fetch(state.endpoint, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(payload),
      signal,
    });

    if (!res.ok) {
      const text = await res.text();
      throw new Error(text || `HTTP ${res.status}`);
    }
    return res.json();
  }

  async function streamChat(payload, assistantEntry) {
    const controller = new AbortController();
    state.controller = controller;
    const res = await fetch(state.endpoint, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(payload),
      signal: controller.signal,
    });

    if (!res.ok) {
      const text = await res.text();
      throw new Error(text || `HTTP ${res.status}`);
    }

    const reader = res.body?.getReader();
    if (!reader) {
      throw new Error("Failed to read streaming response");
    }

    const decoder = new TextDecoder();
    let buffer = "";
    let assembled = "";

    while (true) {
      const { value, done } = await reader.read();
      if (done) break;
      buffer += decoder.decode(value, { stream: true });
      const lines = buffer.split(/\n+/);
      buffer = lines.pop() ?? "";

      for (const line of lines) {
        if (!line.trim()) continue;
        const parsed = safeParse(line);
        if (!parsed) continue;
        if (parsed.error) {
          throw new Error(parsed.error.message || String(parsed.error));
        }
        if (parsed.message?.content) {
          assembled += parsed.message.content;
          updateMessage(assistantEntry, assembled);
        }
        if (parsed.done) {
          buffer = "";
        }
      }
    }

    if (buffer.trim()) {
      const parsed = safeParse(buffer.trim());
      if (parsed?.message?.content) {
        assembled += parsed.message.content;
        updateMessage(assistantEntry, assembled);
      }
    }

    return assembled;
  }

  function renderProviderButtons() {
    dom.providerButtons?.forEach((btn) => {
      const value = btn.dataset.provider;
      if (value === state.providerFilter) {
        btn.classList.add("segmented__btn--active");
      } else {
        btn.classList.remove("segmented__btn--active");
      }
    });
  }

  function setProviderFilter(filter) {
    if (!filter) return;
    state.providerFilter = filter;
    renderProviderButtons();
    renderModels(currentSession()?.modelId);
    const session = currentSession();
    if (session) {
      session.modelScope = filter;
      touchSession(session);
      persistSessions();
      updateSessionHeader(session);
    }
  }

  async function handleSubmit(event) {
    event.preventDefault();
    if (state.loading) return;

    const text = dom.chatInput.value.trim();
    if (!text) return;

    clearError();
    const userEntry = addMessage("user", text);
    updateSessionTitleFrom(userEntry);
    dom.chatInput.value = "";

    const payload = {
      model: selectedModel(),
      messages: buildMessagesForRequest(),
      stream: dom.streamToggle?.checked ?? false,
    };

    try {
      setLoading(true, { streaming: payload.stream });
      let assistantContent = "";
      if (payload.stream) {
        const assistantEntry = addMessage("assistant", "…", { model: payload.model });
        state.pendingAssistant = assistantEntry;
        assistantContent = await streamChat(payload, assistantEntry);
        if (!assistantContent) {
          updateMessage(assistantEntry, "(Empty response)");
        }
      } else {
        const body = await postChat(payload);
        assistantContent = body?.message?.content ?? "(Empty response)";
        addMessage("assistant", assistantContent, { model: payload.model });
      }
      setStatus(`Response from model ${payload.model}`, "online");
    } catch (err) {
      const message = err?.name === "AbortError" ? "Request aborted" : err?.message || String(err);
      if (payload.stream && state.pendingAssistant) {
        updateMessage(state.pendingAssistant, `Error: ${message}`);
      }
      showError(message);
      setStatus("An error occurred", "error");
    } finally {
      state.pendingAssistant = null;
      setLoading(false);
      state.controller = null;
      dom.chatInput.focus();
    }
  }

  function handleStop() {
    if (state.controller) {
      state.controller.abort();
    }
  }

  function buildSamplePayload() {
    const model = selectedModel();
    const system = dom.systemPrompt?.value.trim();
    const lastUser = [...state.history].reverse().find((msg) => msg.role === "user");
    const messages = [];
    if (dom.appendSystem?.checked && system) {
      messages.push({ role: "system", content: system });
    }
    messages.push(
      lastUser || {
        role: "user",
        content: "Hello. Testing response through the router.",
      },
    );
    return { model, messages, stream: dom.streamToggle?.checked ?? false };
  }

  async function handleCopyCurl() {
    try {
      const payload = buildSamplePayload();
      const body = JSON.stringify(payload, null, 2).replace(/'/g, "'\\''");
      const curl = `curl -X POST ${window.location.origin}${state.endpoint} \\
  -H 'Content-Type: application/json' \\
  -d '${body}'`;
      await navigator.clipboard.writeText(curl);
      if (dom.modelCount) {
        const original = dom.modelCount.textContent;
        dom.modelCount.textContent = "cURL copied";
        setTimeout(() => {
          dom.modelCount.textContent = original;
        }, 1500);
      }
    } catch (err) {
      showError(`Failed to copy to clipboard: ${err.message ?? err}`);
    }
  }

  function handleKeydown(event) {
    if (event.key === "Enter" && !event.shiftKey) {
      event.preventDefault();
      dom.chatForm?.requestSubmit();
    }
  }

  function initEvents() {
    dom.chatForm?.addEventListener("submit", handleSubmit);
    dom.chatInput?.addEventListener("keydown", handleKeydown);
    dom.stopButton?.addEventListener("click", handleStop);
    dom.resetButton?.addEventListener("click", () => {
      clearHistory();
      clearError();
    });
    dom.copyCurl?.addEventListener("click", handleCopyCurl);
    dom.modelSelect?.addEventListener("change", () => {
      selectedModel();
      updateSessionHeader(currentSession());
    });
    dom.newChat?.addEventListener("click", () => createSession());
    dom.newChatInline?.addEventListener("click", () => createSession());
    if (dom.sessionList) {
      dom.sessionList.addEventListener("click", (event) => {
        const target = event.target.closest("[data-session-id]");
        if (target?.dataset.sessionId) {
          setActiveSession(target.dataset.sessionId);
        }
      });
    }
    dom.providerButtons?.forEach((btn) => {
      btn.addEventListener("click", () => setProviderFilter(btn.dataset.provider));
    });
  }

  document.addEventListener("DOMContentLoaded", () => {
    setStatus("Connecting...", "connecting");
    hydrateSessions();
    ensureActiveSession();
    renderSessionList();
    renderProviderButtons();
    renderModels(currentSession()?.modelId);
    renderHistory(state.history);
    updateSessionHeader(currentSession());
    initEvents();
    loadModels();
    if (dom.systemPrompt) {
      dom.systemPrompt.value =
        "You are a chat assistant connected to LLM Router. Answer concisely.";
    }
  });
})();
