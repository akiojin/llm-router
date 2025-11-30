(() => {
  const fallbackModels = [
    "gpt-oss:20b",
    "gpt-oss:7b",
    "gpt-oss:3b",
    "gpt-oss:1b",
    "gpt-oss-safeguard:20b",
    "qwen3-coder:30b",
  ];

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
    copyCurl: document.getElementById("copy-curl"),
    messageTemplate: document.getElementById("message-template"),
  };

  const state = {
    history: [],
    models: [],
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

  function formatTime(date) {
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

  function buildModelHint() {
    if (state.models.length) {
      return `${state.models.length} models`;
    }
    return "Using default model";
  }

  function renderModels(models) {
    if (!dom.modelSelect) return;
    dom.modelSelect.innerHTML = "";
    models.forEach((model, index) => {
      const option = document.createElement("option");
      option.value = model;
      option.textContent = model;
      if (index === 0) option.selected = true;
      dom.modelSelect.appendChild(option);
    });
    if (dom.modelCount) dom.modelCount.textContent = buildModelHint();
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
      renderModels(models);
      setStatus("Connected to router", "online");
    } catch (err) {
      state.models = [...fallbackModels];
      renderModels(state.models);
      setStatus("Failed to fetch model list", "error");
      showError(`Failed to fetch model list: ${err.message ?? err}`);
    }
  }

  function clearHistory() {
    state.history = [];
    if (dom.chatLog) {
      dom.chatLog.innerHTML =
        '<div class="chat-empty">No messages yet. Send a message using the input below.</div>';
    }
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
      return `${dom.modelSelect?.value || "model"} · ${time}`;
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
  }

  function addMessage(role, content) {
    const entry = {
      id: crypto.randomUUID ? crypto.randomUUID() : `msg-${Date.now()}-${Math.random()}`,
      role,
      content,
      createdAt: new Date(),
      element: null,
    };
    state.history.push(entry);
    renderMessage(entry);
    return entry;
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
    return dom.modelSelect?.value?.trim() || state.models[0] || "gpt-oss:7b";
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
          buffer = ""; // ignore trailing chunk after done
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

  async function handleSubmit(event) {
    event.preventDefault();
    if (state.loading) return;

    const text = dom.chatInput.value.trim();
    if (!text) return;

    clearError();
    const userEntry = addMessage("user", text);
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
        const assistantEntry = addMessage("assistant", "…");
        state.pendingAssistant = assistantEntry;
        assistantContent = await streamChat(payload, assistantEntry);
        if (!assistantContent) {
          updateMessage(assistantEntry, "(Empty response)");
        }
      } else {
        const body = await postChat(payload);
        assistantContent = body?.message?.content ?? "(Empty response)";
        addMessage("assistant", assistantContent);
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
      const curl = `curl -X POST ${window.location.origin}${state.endpoint} \\\n  -H 'Content-Type: application/json' \\\n  -d '${body}'`;
      await navigator.clipboard.writeText(curl);
      if (dom.modelCount) {
        const original = buildModelHint();
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
  }

  document.addEventListener("DOMContentLoaded", () => {
    initEvents();
    clearHistory();
    loadModels();
    dom.systemPrompt.value = "You are a chat assistant connected to LLM Router. Answer concisely.";
  });
})();
