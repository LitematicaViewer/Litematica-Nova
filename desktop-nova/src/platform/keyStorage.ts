import { invoke } from "./tauri";

export interface AiPublicConfig {
  provider: string;
  base_url: string;
  model: string;
  has_key: boolean;
  key_status: string;
  storage_note: string;
}

export interface AiSaveConfigInput {
  provider: string;
  base_url: string;
  model: string;
  api_key?: string | null;
}

export interface AiTestResult {
  ok: boolean;
  message: string;
}

export interface AiChatMessageWire {
  role: "system" | "user" | "assistant";
  content: string;
}

export interface AiChatCompletionInput {
  messages: AiChatMessageWire[];
}

export interface AiChatCompletionOutput {
  provider: string;
  model: string;
  content: string;
}

export function aiGetConfig(): Promise<AiPublicConfig> {
  return invoke("ai_get_config");
}

export function aiSaveConfig(input: AiSaveConfigInput): Promise<AiPublicConfig> {
  return invoke("ai_save_config", { input });
}

export function aiClearKey(): Promise<AiPublicConfig> {
  return invoke("ai_clear_key");
}

export function aiTestConnection(): Promise<AiTestResult> {
  return invoke("ai_test_connection");
}

export function aiChatCompletion(input: AiChatCompletionInput): Promise<AiChatCompletionOutput> {
  return invoke("ai_chat_completion", { input });
}
