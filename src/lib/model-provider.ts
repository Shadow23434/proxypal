export interface ProviderModelLike {
  id: string;
  ownedBy: string;
  source: string;
}

export const normalizeProvider = (provider: string): string => {
  const normalized = provider.toLowerCase().replace(/[_\s]+/g, "-");

  if (normalized === "github-copilot" || normalized === "copilot") {
    return "copilot";
  }

  return normalized;
};

export const deriveModelProvider = (model: ProviderModelLike): string => {
  const id = model.id.toLowerCase();
  const source = model.source.toLowerCase();
  const ownedBy = model.ownedBy.toLowerCase();

  if (
    source === "copilot" ||
    ownedBy === "copilot" ||
    id.startsWith("github-copilot/") ||
    id.startsWith("copilot-")
  ) {
    return "copilot";
  }

  if (id.includes("antigravity") || id.startsWith("antigravity-")) {
    return "antigravity";
  }

  return ownedBy;
};

export const isModelForProvider = (model: ProviderModelLike, provider: string): boolean => {
  const normalizedProvider = normalizeProvider(provider);
  const modelId = model.id.toLowerCase();
  const source = model.source.toLowerCase();
  const ownedBy = model.ownedBy.toLowerCase();
  const derivedProvider = deriveModelProvider(model);

  if (normalizedProvider === "copilot") {
    return derivedProvider === "copilot";
  }
  if (normalizedProvider === "claude") {
    return ownedBy === "anthropic";
  }
  if (normalizedProvider === "gemini-web") {
    return source.includes("gemini-web");
  }
  if (normalizedProvider === "gemini") {
    return ownedBy === "google" && !source.includes("gemini-web") && source !== "vertex";
  }
  if (normalizedProvider === "vertex") {
    return source.includes("vertex");
  }
  if (normalizedProvider === "codex") {
    return ownedBy === "openai" && source !== "copilot";
  }
  if (normalizedProvider === "qwen") {
    return ownedBy === "qwen" || modelId.includes("qwen");
  }
  if (normalizedProvider === "iflow") {
    return source === "iflow" || ownedBy === "iflow" || modelId.includes("iflow");
  }
  if (normalizedProvider === "antigravity") {
    return derivedProvider === "antigravity" || source === "antigravity" || ownedBy === "antigravity";
  }
  if (normalizedProvider === "kimi") {
    return ownedBy === "kimi" || modelId.includes("kimi");
  }
  if (normalizedProvider === "deepseek") {
    return ownedBy === "deepseek" || modelId.includes("deepseek");
  }

  return derivedProvider === normalizedProvider || ownedBy === normalizedProvider || source === normalizedProvider;
};
