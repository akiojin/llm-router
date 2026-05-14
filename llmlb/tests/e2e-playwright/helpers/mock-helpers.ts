import { type Page } from '@playwright/test';

export function mockOpenAIModels(page: Page): Promise<void> {
  const created = Math.floor(Date.now() / 1000);
  return page.route('**/v1/models', async (route) => {
    await route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({
        object: 'list',
        data: [
          {
            id: 'openai:gpt-4o',
            object: 'model',
            created,
            owned_by: 'openai',
            capabilities: {
              chat_completion: true,
              completion: false,
              embeddings: false,
              fine_tune: false,
              inference: true,
              text_to_speech: false,
              speech_to_text: true,
              image_generation: true,
            },
            lifecycle_status: 'registered',
            download_progress: null,
            ready: true,
          },
        ],
      }),
    });
  }).then(() => undefined);
}

export function mockChatCompletionsStream(
  page: Page,
  assistantText: string
): Promise<void> {
  return page.route('**/v1/chat/completions', async (route) => {
    await route.fulfill({
      status: 200,
      headers: { 'Content-Type': 'text/event-stream' },
      body:
        `data: ${JSON.stringify({ choices: [{ delta: { content: assistantText } }] })}\n\n` +
        'data: [DONE]\n\n',
    });
  }).then(() => undefined);
}
