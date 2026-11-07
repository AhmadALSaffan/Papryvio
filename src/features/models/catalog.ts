import type { ProviderKind } from '../../shared/types/generated/ProviderKind';
import anthropic from '../../assets/providers/anthropic.svg';
import cerebras from '../../assets/providers/cerebras-color.svg';
import cohere from '../../assets/providers/cohere-color.svg';
import deepseek from '../../assets/providers/deepseek-color.svg';
import fireworks from '../../assets/providers/fireworks-color.svg';
import gemini from '../../assets/providers/gemini-color.svg';
import github from '../../assets/providers/github.svg';
import githubcopilot from '../../assets/providers/githubcopilot.svg';
import groq from '../../assets/providers/groq.svg';
import huggingface from '../../assets/providers/huggingface-color.svg';
import kimi from '../../assets/providers/kimi.svg';
import litellm from '../../assets/providers/litellm.svg';
import mistral from '../../assets/providers/mistral-color.svg';
import nvidia from '../../assets/providers/nvidia-color.svg';
import ollama from '../../assets/providers/ollama.svg';
import openai from '../../assets/providers/openai.svg';
import openrouter from '../../assets/providers/openrouter.svg';
import qwen from '../../assets/providers/qwen-color.svg';
import together from '../../assets/providers/together-color.svg';
import xai from '../../assets/providers/xai.svg';
import zai from '../../assets/providers/zai.svg';

/** A brand mark: `mono` marks are single-colour and are drawn in the current text colour. */
export interface Logo {
  src: string;
  mono: boolean;
}

export interface ApiPreset {
  id: string;
  label: string;
  kind: ProviderKind;
  url: string;
  keyUrl?: string;
  logo?: Logo;
  /** A local gateway may run without a key. */
  keyOptional?: boolean;
}

const m = (src: string): Logo => ({ src, mono: true });
const c = (src: string): Logo => ({ src, mono: false });

export const SIGN_IN_LOGOS = { chatgpt: m(openai), github_copilot: m(githubcopilot) } as const;
export const OLLAMA_LOGO = m(ollama);

/** Hosted APIs reached with the user's own key (OpenAI-compatible unless noted). */
export const API_PRESETS: ApiPreset[] = [
  { id: 'openai', label: 'OpenAI', kind: 'openai_compatible', url: 'https://api.openai.com/v1', keyUrl: 'https://platform.openai.com/api-keys', logo: m(openai) },
  { id: 'anthropic', label: 'Anthropic', kind: 'anthropic', url: 'https://api.anthropic.com', keyUrl: 'https://console.anthropic.com/settings/keys', logo: m(anthropic) },
  { id: 'gemini', label: 'Google Gemini', kind: 'openai_compatible', url: 'https://generativelanguage.googleapis.com/v1beta/openai', keyUrl: 'https://aistudio.google.com/apikey', logo: c(gemini) },
  { id: 'xai', label: 'xAI Grok', kind: 'openai_compatible', url: 'https://api.x.ai/v1', keyUrl: 'https://console.x.ai/', logo: m(xai) },
  { id: 'mistral', label: 'Mistral', kind: 'openai_compatible', url: 'https://api.mistral.ai/v1', keyUrl: 'https://console.mistral.ai/api-keys', logo: c(mistral) },
  { id: 'deepseek', label: 'DeepSeek', kind: 'openai_compatible', url: 'https://api.deepseek.com/v1', keyUrl: 'https://platform.deepseek.com/api_keys', logo: c(deepseek) },
  { id: 'openrouter', label: 'OpenRouter', kind: 'openai_compatible', url: 'https://openrouter.ai/api/v1', keyUrl: 'https://openrouter.ai/settings/keys', logo: m(openrouter) },
  { id: 'groq', label: 'Groq', kind: 'openai_compatible', url: 'https://api.groq.com/openai/v1', keyUrl: 'https://console.groq.com/keys', logo: m(groq) },
  { id: 'together', label: 'Together AI', kind: 'openai_compatible', url: 'https://api.together.xyz/v1', keyUrl: 'https://api.together.ai/settings/api-keys', logo: c(together) },
  { id: 'fireworks', label: 'Fireworks AI', kind: 'openai_compatible', url: 'https://api.fireworks.ai/inference/v1', keyUrl: 'https://fireworks.ai/account/api-keys', logo: c(fireworks) },
  { id: 'cerebras', label: 'Cerebras', kind: 'openai_compatible', url: 'https://api.cerebras.ai/v1', keyUrl: 'https://cloud.cerebras.ai/', logo: c(cerebras) },
  { id: 'github', label: 'GitHub Models', kind: 'openai_compatible', url: 'https://models.github.ai/inference', keyUrl: 'https://github.com/settings/personal-access-tokens', logo: m(github) },
  { id: 'huggingface', label: 'Hugging Face', kind: 'openai_compatible', url: 'https://router.huggingface.co/v1', keyUrl: 'https://huggingface.co/settings/tokens', logo: c(huggingface) },
  { id: 'nvidia', label: 'NVIDIA NIM', kind: 'openai_compatible', url: 'https://integrate.api.nvidia.com/v1', keyUrl: 'https://build.nvidia.com/', logo: c(nvidia) },
  { id: 'moonshot', label: 'Moonshot Kimi', kind: 'openai_compatible', url: 'https://api.moonshot.ai/v1', keyUrl: 'https://platform.moonshot.ai/console/api-keys', logo: m(kimi) },
  { id: 'qwen', label: 'Qwen (Alibaba Cloud)', kind: 'openai_compatible', url: 'https://dashscope-intl.aliyuncs.com/compatible-mode/v1', keyUrl: 'https://modelstudio.console.alibabacloud.com/', logo: c(qwen) },
  { id: 'zai', label: 'Z.ai GLM', kind: 'openai_compatible', url: 'https://api.z.ai/api/paas/v4', keyUrl: 'https://z.ai/manage-apikey/apikey-list', logo: m(zai) },
  { id: 'cohere', label: 'Cohere', kind: 'openai_compatible', url: 'https://api.cohere.ai/compatibility/v1', keyUrl: 'https://dashboard.cohere.com/api-keys', logo: c(cohere) },
  { id: 'custom', label: 'OpenAI-compatible', kind: 'openai_compatible', url: 'https://' },
];

export const LITELLM_PRESET: ApiPreset = {
  id: 'litellm',
  label: 'LiteLLM',
  kind: 'openai_compatible',
  url: 'http://127.0.0.1:4000',
  keyUrl: 'https://docs.litellm.ai/docs/simple_proxy',
  logo: m(litellm),
  keyOptional: true,
};

function host(url: string): string {
  try {
    return new URL(url).host;
  } catch {
    return '';
  }
}

/** The mark for a connected provider, from its kind and address. */
export function logoFor(kind: ProviderKind, baseUrl: string, label: string): Logo | undefined {
  if (kind === 'ollama') return OLLAMA_LOGO;
  if (kind === 'chatgpt') return SIGN_IN_LOGOS.chatgpt;
  if (kind === 'github_copilot') return SIGN_IN_LOGOS.github_copilot;
  if (kind === 'anthropic') return m(anthropic);
  const h = host(baseUrl);
  const preset = API_PRESETS.find((p) => p.logo && h !== '' && host(p.url) === h);
  if (preset) return preset.logo;
  if (/litellm/i.test(label)) return LITELLM_PRESET.logo;
  return undefined;
}
