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
  const SETTINGS_KEY = "router:chat:settings:v1";

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
    errorMessage: document.getElementById("error-message"),
    errorClose: document.getElementById("error-close"),
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
    settingsToggle: document.getElementById("settings-toggle"),
    settingsModal: document.getElementById("settings-modal"),
    modalClose: document.getElementById("modal-close"),
    sidebar: document.getElementById("sidebar"),
    sidebarToggle: document.getElementById("sidebar-toggle"),
    sidebarToggleMobile: document.getElementById("sidebar-toggle-mobile"),
    apiKeyInput: document.getElementById("api-key-input"),
  };
  dom.providerButtons = dom.providerToggle
    ? Array.from(dom.providerToggle.querySelectorAll(".provider-btn"))
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
    el.title = text;

    el.classList.remove(
      "status-indicator--connecting",
      "status-indicator--offline",
      "status-indicator--online",
      "status-indicator--error",
    );

    const map = {
      online: "status-indicator--online",
      offline: "status-indicator--offline",
      connecting: "status-indicator--connecting",
      error: "status-indicator--error",
    };

    el.classList.add(map[variant] ?? "status-indicator--online");
  }

  function showError(message) {
    if (!dom.errorBanner) return;
    if (dom.errorMessage) {
      dom.errorMessage.textContent = message;
    } else {
      dom.errorBanner.textContent = message;
    }
    dom.errorBanner.classList.remove("hidden");
  }

  function clearError() {
    dom.errorBanner?.classList.add("hidden");
    if (dom.errorMessage) dom.errorMessage.textContent = "";
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
      const res = await fetch("/api/models/available");
      if (!res.ok) {
        throw new Error(`HTTP ${res.status}`);
      }
      const body = await res.json();
      const models = (body?.models || []).map((item) => item.name).filter(Boolean);
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
          reasoning: msg.reasoning || null,
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
        title: session.title || "新規Playground",
        modelId: session.modelId,
        modelScope: session.modelScope || "local",
        createdAt: session.createdAt || new Date().toISOString(),
        updatedAt: session.updatedAt || session.createdAt || new Date().toISOString(),
        history: (session.history || []).map((msg) => ({
          role: msg.role,
          content: msg.content,
          reasoning: msg.reasoning || null,
          model: msg.model,
          createdAt: msg.createdAt ? new Date(msg.createdAt) : new Date(),
          element: null,
        })),
      }));
    } catch (_err) {
      state.sessions = [];
    }
  }

  function persistSettings() {
    try {
      const settings = {
        streamEnabled: dom.streamToggle?.checked ?? false,
        appendSystem: dom.appendSystem?.checked ?? true,
        apiKey: dom.apiKeyInput?.value || "",
      };
      localStorage.setItem(SETTINGS_KEY, JSON.stringify(settings));
    } catch (_err) {
      // 永続化失敗は致命的ではないため無視
    }
  }

  function hydrateSettings() {
    try {
      const raw = localStorage.getItem(SETTINGS_KEY);
      if (!raw) return;
      const settings = JSON.parse(raw);
      if (dom.streamToggle && typeof settings.streamEnabled === "boolean") {
        dom.streamToggle.checked = settings.streamEnabled;
      }
      if (dom.appendSystem && typeof settings.appendSystem === "boolean") {
        dom.appendSystem.checked = settings.appendSystem;
      }
      if (dom.apiKeyInput && typeof settings.apiKey === "string") {
        dom.apiKeyInput.value = settings.apiKey;
      }
    } catch (_err) {
      // 復元失敗は無視
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
    if (dom.activeSessionTitle) dom.activeSessionTitle.textContent = session.title || "新規Playground";
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
      empty.textContent = "まだセッションがありません。新規Playgroundを作成してください。";
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
      title.textContent = session.title || "新規Playground";

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
        '<div class="chat-welcome"><h2>LLM Router Chat</h2><p>Select a model and start chatting</p></div>';
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

  function createSession({ title = "新規Playground", persist = true } = {}) {
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

    const avatarEl = node.querySelector(".message-avatar");
    const roleEl = node.querySelector(".message-role");
    const textEl = node.querySelector(".message-text");
    const metaEl = node.querySelector(".message-meta");

    // Avatar text is handled via CSS ::before pseudo-element
    if (roleEl) roleEl.textContent = messageLabel(entry.role);
    if (textEl) {
      // Reasoning content を折りたたみ表示
      if (entry.reasoning && entry.role === "assistant") {
        const details = document.createElement("details");
        details.className = "reasoning-block";
        const summary = document.createElement("summary");
        summary.className = "reasoning-summary";
        summary.textContent = "思考過程を表示";
        const reasoningContent = document.createElement("div");
        reasoningContent.className = "reasoning-content";
        reasoningContent.textContent = entry.reasoning;
        details.appendChild(summary);
        details.appendChild(reasoningContent);
        textEl.appendChild(details);

        const mainContent = document.createElement("div");
        mainContent.className = "main-content";
        mainContent.textContent = entry.content;
        textEl.appendChild(mainContent);
      } else {
        textEl.textContent = entry.content;
      }
    }
    if (metaEl) metaEl.textContent = messageMeta(entry);

    dom.chatLog.querySelector(".chat-welcome")?.remove();
    dom.chatLog.appendChild(node);
    scrollToBottom(true); // スムーズスクロール
    entry.element = node;
  }

  function getScrollContainer() {
    // chat-messagesの親要素(.chat-container)がスクロール可能
    return dom.chatLog?.parentElement;
  }

  function isNearBottom() {
    const container = getScrollContainer();
    if (!container) return true;
    const threshold = 100; // ピクセル単位のしきい値
    const { scrollTop, scrollHeight, clientHeight } = container;
    return scrollHeight - scrollTop - clientHeight < threshold;
  }

  function scrollToBottom(smooth = false) {
    const container = getScrollContainer();
    if (!container) return;
    // ダブルrequestAnimationFrameでレイアウト完了後にスクロール
    requestAnimationFrame(() => {
      requestAnimationFrame(() => {
        container.scrollTo({
          top: container.scrollHeight,
          behavior: smooth ? "smooth" : "instant",
        });
      });
    });
  }

  function updateMessage(entry, content) {
    entry.content = content;
    if (entry.element) {
      const textEl = entry.element.querySelector(".message-text");
      const metaEl = entry.element.querySelector(".message-meta");
      if (textEl) textEl.textContent = content;
      if (metaEl) metaEl.textContent = messageMeta(entry);
      // ストリーミング中は自動スクロール（ユーザーが下部付近にいる場合のみ）
      if (isNearBottom()) {
        scrollToBottom();
      }
    }
    persistSessions();
  }

  function addMessage(role, content, { model, reasoning } = {}) {
    const entry = {
      id: crypto.randomUUID ? crypto.randomUUID() : `msg-${Date.now()}-${Math.random()}`,
      role,
      content,
      reasoning: reasoning || null,
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

  function addTypingIndicator(model) {
    const template = dom.messageTemplate;
    if (!template || !dom.chatLog) return null;

    const node = template.content.firstElementChild.cloneNode(true);
    const id = `typing-${Date.now()}`;
    node.dataset.messageId = id;
    node.classList.add("message--assistant");

    const roleEl = node.querySelector(".message-role");
    const textEl = node.querySelector(".message-text");
    const metaEl = node.querySelector(".message-meta");

    if (roleEl) roleEl.textContent = "Assistant";
    if (textEl) {
      textEl.innerHTML = '<span class="typing-indicator"><span class="dot"></span><span class="dot"></span><span class="dot"></span></span>';
    }
    if (metaEl) metaEl.textContent = model ? `${model} is typing...` : "Generating...";

    dom.chatLog.querySelector(".chat-welcome")?.remove();
    dom.chatLog.appendChild(node);
    scrollToBottom(true); // スムーズスクロール

    return { id, element: node };
  }

  function removeTypingIndicator(entry) {
    if (entry?.element) {
      entry.element.remove();
    }
  }

  function updateSessionTitleFrom(entry) {
    const session = currentSession();
    if (!session || entry.role !== "user") return;
    const defaultTitle = "新規Playground";
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

  function getAuthHeaders() {
    const headers = { "Content-Type": "application/json" };
    const apiKey = dom.apiKeyInput?.value?.trim();
    if (apiKey) {
      headers["Authorization"] = `Bearer ${apiKey}`;
    }
    return headers;
  }

  async function postChat(payload, signal) {
    const res = await fetch(state.endpoint, {
      method: "POST",
      headers: getAuthHeaders(),
      body: JSON.stringify(payload),
      signal,
    });

    if (!res.ok) {
      const text = await res.text();
      throw new Error(text || `HTTP ${res.status}`);
    }
    return res.json();
  }

  async function streamChat(payload, assistantEntry, typingEntry = null) {
    const controller = new AbortController();
    state.controller = controller;
    const res = await fetch(state.endpoint, {
      method: "POST",
      headers: getAuthHeaders(),
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
    let firstTokenReceived = false;

    while (true) {
      const { value, done } = await reader.read();
      if (done) break;
      buffer += decoder.decode(value, { stream: true });
      const lines = buffer.split(/\n+/);
      buffer = lines.pop() ?? "";

      for (const line of lines) {
        if (!line.trim()) continue;
        // SSE形式の場合は "data: " プレフィックスを除去
        const dataLine = line.startsWith("data: ") ? line.slice(6) : line;
        if (dataLine === "[DONE]") {
          buffer = "";
          continue;
        }
        const parsed = safeParse(dataLine);
        if (!parsed) continue;
        if (parsed.error) {
          throw new Error(parsed.error.message || String(parsed.error));
        }
        // OpenAI互換形式: choices[0].delta.content (ストリーミング)
        const delta = parsed.choices?.[0]?.delta?.content;
        if (delta) {
          // 最初のトークン到着時にタイピングインジケーターを削除し、メッセージを表示
          if (!firstTokenReceived) {
            firstTokenReceived = true;
            removeTypingIndicator(typingEntry);
            if (assistantEntry.element) {
              assistantEntry.element.classList.remove("hidden");
            }
          }
          assembled += delta;
          updateMessage(assistantEntry, assembled);
        }
        // 終了判定
        if (parsed.choices?.[0]?.finish_reason) {
          buffer = "";
        }
      }
    }

    if (buffer.trim()) {
      const dataLine = buffer.trim().startsWith("data: ") ? buffer.trim().slice(6) : buffer.trim();
      if (dataLine !== "[DONE]") {
        const parsed = safeParse(dataLine);
        // OpenAI互換形式: choices[0].delta.content
        const delta = parsed?.choices?.[0]?.delta?.content;
        if (delta) {
          if (!firstTokenReceived) {
            firstTokenReceived = true;
            removeTypingIndicator(typingEntry);
            if (assistantEntry.element) {
              assistantEntry.element.classList.remove("hidden");
            }
          }
          assembled += delta;
          updateMessage(assistantEntry, assembled);
        }
      }
    }

    // 応答が空の場合もタイピングインジケーターを削除
    if (!firstTokenReceived) {
      removeTypingIndicator(typingEntry);
      if (assistantEntry.element) {
        assistantEntry.element.classList.remove("hidden");
      }
    }

    return assembled;
  }

  function renderProviderButtons() {
    dom.providerButtons?.forEach((btn) => {
      const value = btn.dataset.provider;
      if (value === state.providerFilter) {
        btn.classList.add("provider-btn--active");
      } else {
        btn.classList.remove("provider-btn--active");
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
        // ストリーミングモードでもタイピングインジケーターを表示
        const typingEntry = addTypingIndicator(payload.model);
        const assistantEntry = addMessage("assistant", "", { model: payload.model });
        assistantEntry.element.classList.add("hidden"); // 最初は非表示
        state.pendingAssistant = assistantEntry;
        try {
          assistantContent = await streamChat(payload, assistantEntry, typingEntry);
          if (!assistantContent) {
            updateMessage(assistantEntry, "(Empty response)");
          }
        } catch (err) {
          removeTypingIndicator(typingEntry);
          throw err;
        }
      } else {
        // 非ストリーミングモードでもタイピングインジケーターを表示
        const typingEntry = addTypingIndicator(payload.model);
        try {
          const body = await postChat(payload);
          // OpenAI互換形式: choices[0].message.content
          const message = body?.choices?.[0]?.message;
          assistantContent = message?.content ?? "(Empty response)";
          // reasoning_content をサポート（o1系モデル等）
          const reasoning = message?.reasoning_content || null;
          removeTypingIndicator(typingEntry);
          addMessage("assistant", assistantContent, { model: payload.model, reasoning });
        } catch (err) {
          removeTypingIndicator(typingEntry);
          throw err;
        }
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
    // IME変換中のEnterは無視する（日本語入力など）
    if (event.isComposing || event.keyCode === 229) {
      return;
    }
    if (event.key === "Enter" && !event.shiftKey) {
      event.preventDefault();
      dom.chatForm?.requestSubmit();
    }
  }

  function openSettingsModal() {
    if (dom.settingsModal) {
      dom.settingsModal.showModal();
    }
  }

  function closeSettingsModal() {
    if (dom.settingsModal) {
      dom.settingsModal.close();
    }
  }

  function toggleSidebar() {
    if (dom.sidebar) {
      dom.sidebar.classList.toggle("sidebar--collapsed");
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
    dom.streamToggle?.addEventListener("change", persistSettings);
    dom.appendSystem?.addEventListener("change", persistSettings);
    dom.apiKeyInput?.addEventListener("change", persistSettings);
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

    // Settings modal
    dom.settingsToggle?.addEventListener("click", openSettingsModal);
    dom.modalClose?.addEventListener("click", closeSettingsModal);
    dom.settingsModal?.addEventListener("click", (event) => {
      if (event.target === dom.settingsModal) {
        closeSettingsModal();
      }
    });

    // Sidebar toggle
    dom.sidebarToggle?.addEventListener("click", toggleSidebar);
    dom.sidebarToggleMobile?.addEventListener("click", toggleSidebar);

    // Error close button
    dom.errorClose?.addEventListener("click", clearError);
  }

  document.addEventListener("DOMContentLoaded", () => {
    setStatus("Connecting...", "connecting");
    hydrateSessions();
    hydrateSettings();
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
