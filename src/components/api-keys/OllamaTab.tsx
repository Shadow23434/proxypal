import { createEffect, createMemo, createSignal, For, Show, splitProps } from "solid-js";
import { useI18n } from "../../i18n";
import {
  fetchOllamaModels,
  getOllamaProviders,
  reloadConfig,
  setOllamaProviders,
  testOllamaProvider,
} from "../../lib/tauri";
import { appStore } from "../../stores/app";
import { toastStore } from "../../stores/toast";
import { Button } from "../ui";

import type { OllamaProvider } from "../../lib/tauri";

interface OllamaTabProps {
  loading: () => boolean;
  setLoading: (value: boolean) => void;
  setShowAddForm: (value: boolean) => void;
  showAddForm: () => boolean;
}

export function OllamaTab(props: OllamaTabProps) {
  const [local] = splitProps(props, ["showAddForm", "setShowAddForm", "loading", "setLoading"]);
  const { t } = useI18n();
  const { proxyStatus } = appStore;
  const [providers, setProviders] = createSignal<OllamaProvider[]>([]);
  const [editingIndex, setEditingIndex] = createSignal<number | null>(null);
  const [showModelManager, setShowModelManager] = createSignal(false);
  const [managingProviderIndex, setManagingProviderIndex] = createSignal<number | null>(null);
  const [newModelInput, setNewModelInput] = createSignal("");
  const [newProvider, setNewProvider] = createSignal<OllamaProvider>({
    apiKeyEntries: [{ apiKey: "" }],
    baseUrl: "",
    models: [],
    name: "",
  });
  const [testingIndex, setTestingIndex] = createSignal<number | null>(null);
  const [testingNewProvider, setTestingNewProvider] = createSignal(false);
  const [testResult, setTestResult] = createSignal<{ message: string; modelsFound?: number; success: boolean } | null>(null);
  const [fetchingModels, setFetchingModels] = createSignal(false);
  const [bulkAddMode, setBulkAddMode] = createSignal(false);
  const [bulkKeysInput, setBulkKeysInput] = createSignal("");
  const [headersInput, setHeadersInput] = createSignal("");

  const loadKeys = async () => {
    if (!proxyStatus().running) return;
    local.setLoading(true);
    try {
      setProviders(await getOllamaProviders());
    } catch (error) {
      console.error("Failed to load Ollama providers:", error);
      toastStore.error(t("apiKeys.toasts.failedToLoadApiKeys"), String(error));
    } finally {
      local.setLoading(false);
    }
  };

  createEffect(() => {
    if (proxyStatus().running) void loadKeys();
  });

  const enrichProviderModels = async (provider: OllamaProvider) => {
    const providerModels = await fetchOllamaModels([provider]).catch(() => []);
    const matched = providerModels.find((item) => item.providerName === provider.name);
    if (!matched || matched.error || matched.models.length === 0) return provider;
    return {
      ...provider,
      models: matched.models.map((model) => ({ alias: model.id, name: model.id })),
    };
  };

  const formatHeaders = (headers?: Record<string, string>) => {
    if (!headers) return "";
    return Object.entries(headers).map(([key, value]) => `${key}: ${value}`).join("\n");
  };

  const parseHeaders = (raw: string): Record<string, string> | undefined => {
    const entries = raw
      .split("\n")
      .map((line) => line.trim())
      .filter((line) => line.length > 0)
      .map((line) => {
        const separatorIndex = line.indexOf(":");
        if (separatorIndex === -1) return null;
        const key = line.slice(0, separatorIndex).trim();
        const value = line.slice(separatorIndex + 1).trim();
        if (!key || !value) return null;
        return [key, value] as const;
      })
      .filter((entry): entry is [string, string] => entry !== null);
    return entries.length === 0 ? undefined : Object.fromEntries(entries);
  };

  const resetForm = () => {
    setNewProvider({ apiKeyEntries: [{ apiKey: "" }], baseUrl: "", models: [], name: "" });
    setEditingIndex(null);
    local.setShowAddForm(false);
    setBulkAddMode(false);
    setBulkKeysInput("");
    setHeadersInput("");
  };

  const saveProviders = async (nextProviders: OllamaProvider[], successToast?: string) => {
    await setOllamaProviders(nextProviders);
    setProviders(nextProviders);
    if (successToast) toastStore.success(successToast);
    await reloadConfig();
  };

  const handleAddOrUpdate = async () => {
    const provider = newProvider();
    const nextProvider = { ...provider, headers: parseHeaders(headersInput()) };
    if (!nextProvider.name.trim() || !nextProvider.baseUrl.trim()) {
      toastStore.error(t("apiKeys.toasts.nameAndBaseUrlRequired"));
      return;
    }
    if (!nextProvider.apiKeyEntries[0]?.apiKey.trim() && !nextProvider.headers) {
      toastStore.error(t("apiKeys.toasts.atLeastOneApiKeyOrHeaderRequired"));
      return;
    }

    local.setLoading(true);
    try {
      const providerWithModels = await enrichProviderModels(nextProvider);
      if (editingIndex() === null) {
        await saveProviders([...providers(), providerWithModels], t("apiKeys.toasts.providerAdded"));
      } else {
        await saveProviders(providers().map((p, i) => (i === editingIndex() ? providerWithModels : p)), t("apiKeys.toasts.providerUpdated"));
      }
      resetForm();
    } catch (error) {
      toastStore.error(editingIndex() === null ? t("apiKeys.toasts.failedToAddProvider") : t("apiKeys.toasts.failedToUpdateProvider"), String(error));
    } finally {
      local.setLoading(false);
    }
  };

  const handleDeleteProvider = async (index: number) => {
    local.setLoading(true);
    try {
      await saveProviders(providers().filter((_, i) => i !== index), t("apiKeys.toasts.providerDeleted"));
    } catch (error) {
      toastStore.error(t("apiKeys.toasts.failedToDeleteProvider"), String(error));
    } finally {
      local.setLoading(false);
    }
  };

  const handleEditProvider = (index: number) => {
    const provider = providers()[index];
    setEditingIndex(index);
    setNewProvider({ ...provider, apiKeyEntries: [...provider.apiKeyEntries] });
    setHeadersInput(formatHeaders(provider.headers));
    if (provider.apiKeyEntries.length > 1) {
      setBulkAddMode(true);
      setBulkKeysInput(provider.apiKeyEntries.map((entry) => entry.apiKey).filter((apiKey) => apiKey.trim()).join("\n"));
    } else {
      setBulkAddMode(false);
      setBulkKeysInput("");
    }
    local.setShowAddForm(true);
  };

  const handleOpenModelManager = (index: number) => {
    setManagingProviderIndex(index);
    setShowModelManager(true);
  };

  const handleAddModel = async () => {
    const model = newModelInput().trim();
    const index = managingProviderIndex();
    if (!model || index === null) return;
    const updated = providers().map((p, i) => {
      if (i !== index) return p;
      const existingModels = p.models || [];
      const alreadyExists = existingModels.some((existingModel) => existingModel.name === model || existingModel.alias === model);
      return alreadyExists ? p : { ...p, models: [...existingModels, { alias: model, name: model }] };
    });
    local.setLoading(true);
    try {
      await saveProviders(updated);
      setNewModelInput("");
    } catch (error) {
      toastStore.error(t("apiKeys.toasts.failedToAddModel"), String(error));
    } finally {
      local.setLoading(false);
    }
  };

  const handleRemoveModel = async (modelIndex: number) => {
    const providerIndex = managingProviderIndex();
    if (providerIndex === null) return;
    const updated = providers().map((p, i) => i === providerIndex && p.models ? { ...p, models: p.models.filter((_, index) => index !== modelIndex) } : p);
    local.setLoading(true);
    try {
      await saveProviders(updated, t("apiKeys.toasts.modelRemoved"));
    } catch (error) {
      toastStore.error(t("apiKeys.toasts.failedToRemoveModel"), String(error));
    } finally {
      local.setLoading(false);
    }
  };

  const handleSaveModels = async () => {
    local.setLoading(true);
    try {
      await saveProviders(providers(), t("apiKeys.toasts.modelsSaved"));
      setShowModelManager(false);
      setManagingProviderIndex(null);
    } catch (error) {
      toastStore.error(t("apiKeys.toasts.failedToSaveModels"), String(error));
    } finally {
      local.setLoading(false);
    }
  };

  const handleFetchModels = async () => {
    const index = managingProviderIndex();
    if (index === null) return;
    setFetchingModels(true);
    try {
      const allProviderModels = await fetchOllamaModels();
      const provider = providers()[index];
      const providerModels = allProviderModels.find((item) => item.providerName === provider.name);
      if (providerModels?.error) {
        toastStore.error(t("apiKeys.toasts.failedToFetchModels"), providerModels.error);
        return;
      }
      if (!providerModels || providerModels.models.length === 0) {
        toastStore.warning(t("apiKeys.toasts.noModelsFound"), t("apiKeys.toasts.providerReturnedNoModels"));
        return;
      }
      const existingModelNames = new Set((provider.models || []).map((model) => model.name));
      const newModels = providerModels.models.filter((model) => !existingModelNames.has(model.id));
      if (newModels.length === 0) {
        toastStore.info(t("apiKeys.toasts.noNewModels"), t("apiKeys.toasts.allFetchedModelsAlreadyExist"));
        return;
      }
      const updated = providers().map((p, i) => i === index ? { ...p, models: [...(p.models || []), ...newModels.map((model) => ({ alias: model.id, name: model.id }))] } : p);
      await saveProviders(updated);
      toastStore.success(t("apiKeys.toasts.addedModels", { count: newModels.length }), t("apiKeys.toasts.totalModels", { count: updated[index].models?.length || 0 }));
    } catch (error) {
      toastStore.error(t("apiKeys.toasts.failedToFetchModels"), String(error));
    } finally {
      setFetchingModels(false);
    }
  };

  const handleTestProvider = async (baseUrl: string, apiKey: string, headers?: Record<string, string>, index?: number) => {
    if (index !== undefined) setTestingIndex(index); else setTestingNewProvider(true);
    setTestResult(null);
    try {
      const result = await testOllamaProvider(baseUrl, apiKey, headers);
      setTestResult({ message: result.message, modelsFound: result.modelsFound ?? undefined, success: result.success });
      if (result.success) {
        toastStore.success(t("apiKeys.toasts.connectionSuccessful"), result.modelsFound ? t("apiKeys.toasts.foundModels", { count: result.modelsFound }) : undefined);
      } else {
        toastStore.error(t("apiKeys.toasts.connectionFailed"), result.message);
      }
    } catch (error) {
      setTestResult({ message: String(error), success: false });
      toastStore.error(t("apiKeys.toasts.testFailed"), String(error));
    } finally {
      setTestingIndex(null);
      setTestingNewProvider(false);
    }
  };

  const showEmptyState = createMemo(() => proxyStatus().running && !local.loading() && providers().length === 0 && !local.showAddForm());

  return (
    <div class="space-y-4">
      <p class="text-xs text-gray-500 dark:text-gray-400">{t("apiKeys.ollamaDescription")}</p>
      <Show when={providers().length > 0}>
        <div class="space-y-2">
          <For each={providers()}>
            {(provider, index) => (
              <div class="rounded-lg border border-gray-200 bg-gray-50 p-3 dark:border-gray-700 dark:bg-gray-800/50">
                <div class="flex items-center justify-between">
                  <div class="min-w-0 flex-1">
                    <p class="text-sm font-medium text-gray-900 dark:text-gray-100">{provider.name}</p>
                    <p class="truncate text-xs text-gray-500 dark:text-gray-400">{provider.baseUrl}</p>
                    <p class="mt-1 text-xs text-gray-400 dark:text-gray-500">
                      {t("apiKeys.apiKeysCount", { count: provider.apiKeyEntries.length })}
                      {provider.headers && <span class="ml-2 text-gray-400 dark:text-gray-500">{t("apiKeys.headersCount", { count: Object.keys(provider.headers).length })}</span>}
                    </p>
                  </div>
                  <div class="flex items-center gap-1">
                    <Button disabled={testingIndex() === index()} onClick={() => handleTestProvider(provider.baseUrl, provider.apiKeyEntries[0]?.apiKey || "", provider.headers, index())} size="sm" title={t("apiKeys.actions.testConnection")} variant="ghost">Test</Button>
                    <Button onClick={() => handleEditProvider(index())} size="sm" title={t("apiKeys.actions.editProvider")} variant="ghost">Edit</Button>
                    <Button onClick={() => handleOpenModelManager(index())} size="sm" title={t("apiKeys.actions.manageModels")} variant="ghost">Models</Button>
                    <Button onClick={() => handleDeleteProvider(index())} size="sm" title={t("apiKeys.actions.deleteProvider")} variant="ghost">Delete</Button>
                  </div>
                </div>
              </div>
            )}
          </For>
        </div>
      </Show>

      <Show when={local.showAddForm()}>
        <div class="space-y-3 rounded-xl border border-gray-200 bg-gray-50 p-4 dark:border-gray-700 dark:bg-gray-800/50">
          <label class="block">
            <span class="text-sm font-medium text-gray-700 dark:text-gray-300">{t("apiKeys.labels.providerNameRequired")}</span>
            <input class="mt-1 block w-full rounded-lg border border-gray-300 bg-white px-3 py-2 text-sm focus:border-transparent focus:ring-2 focus:ring-brand-500 dark:border-gray-600 dark:bg-gray-900" onInput={(e) => setNewProvider({ ...newProvider(), name: e.currentTarget.value })} placeholder={t("apiKeys.placeholders.providerName")} type="text" value={newProvider().name} />
          </label>
          <label class="block">
            <span class="text-sm font-medium text-gray-700 dark:text-gray-300">{t("apiKeys.labels.baseUrlRequired")}</span>
            <input class="mt-1 block w-full rounded-lg border border-gray-300 bg-white px-3 py-2 text-sm focus:border-transparent focus:ring-2 focus:ring-brand-500 dark:border-gray-600 dark:bg-gray-900" onInput={(e) => setNewProvider({ ...newProvider(), baseUrl: e.currentTarget.value })} placeholder={t("apiKeys.placeholders.ollamaBaseUrl")} type="text" value={newProvider().baseUrl} />
          </label>
          <div class="space-y-2">
            <div class="flex items-center justify-between">
              <span class="text-sm font-medium text-gray-700 dark:text-gray-300">{t("apiKeys.labels.apiKeysOptional")}</span>
              <button class="text-xs text-brand-600 hover:underline dark:text-brand-400" onClick={() => {
                setBulkAddMode(!bulkAddMode());
                if (!bulkAddMode()) setBulkKeysInput(newProvider().apiKeyEntries.map((entry) => entry.apiKey).filter((apiKey) => apiKey.trim()).join("\n"));
              }} type="button">{bulkAddMode() ? t("apiKeys.actions.singleKey") : t("apiKeys.actions.bulkAdd")}</button>
            </div>
            <Show when={!bulkAddMode()}>
              <input class="block w-full rounded-lg border border-gray-300 bg-white px-3 py-2 text-sm focus:border-transparent focus:ring-2 focus:ring-brand-500 dark:border-gray-600 dark:bg-gray-900" onInput={(e) => setNewProvider({ ...newProvider(), apiKeyEntries: [{ apiKey: e.currentTarget.value }] })} placeholder={t("apiKeys.placeholders.providerApiKey")} type="password" value={newProvider().apiKeyEntries[0]?.apiKey || ""} />
            </Show>
            <Show when={bulkAddMode()}>
              <textarea class="block w-full rounded-lg border border-gray-300 bg-white px-3 py-2 font-mono text-sm focus:border-transparent focus:ring-2 focus:ring-brand-500 dark:border-gray-600 dark:bg-gray-900" onInput={(e) => {
                setBulkKeysInput(e.currentTarget.value);
                const keys = e.currentTarget.value.split("\n").map((key) => key.trim()).filter((key) => key.length > 0).map((apiKey) => ({ apiKey }));
                setNewProvider({ ...newProvider(), apiKeyEntries: keys.length > 0 ? keys : [{ apiKey: "" }] });
              }} placeholder={t("apiKeys.placeholders.bulkApiKeys")} rows={5} value={bulkKeysInput()} />
            </Show>
          </div>
          <label class="block">
            <span class="text-sm font-medium text-gray-700 dark:text-gray-300">{t("apiKeys.labels.headersOptional")}</span>
            <textarea class="mt-1 block w-full rounded-lg border border-gray-300 bg-white px-3 py-2 font-mono text-xs focus:border-transparent focus:ring-2 focus:ring-brand-500 dark:border-gray-600 dark:bg-gray-900" onInput={(e) => setHeadersInput(e.currentTarget.value)} placeholder={t("apiKeys.placeholders.providerHeaders")} rows={3} value={headersInput()} />
          </label>
          <label class="block">
            <span class="text-sm font-medium text-gray-700 dark:text-gray-300">{t("apiKeys.labels.prefixOptional")}</span>
            <input class="mt-1 block w-full rounded-lg border border-gray-300 bg-white px-3 py-2 text-sm focus:border-transparent focus:ring-2 focus:ring-brand-500 dark:border-gray-600 dark:bg-gray-900" onInput={(e) => setNewProvider({ ...newProvider(), prefix: e.currentTarget.value || undefined })} placeholder={t("apiKeys.placeholders.providerPrefix")} type="text" value={newProvider().prefix || ""} />
          </label>
          <div class="flex gap-2 pt-2">
            <Button disabled={local.loading()} onClick={handleAddOrUpdate} size="sm" variant="primary">{editingIndex() !== null ? t("apiKeys.actions.updateProvider") : t("apiKeys.actions.addProvider")}</Button>
            <Button disabled={testingNewProvider() || !newProvider().baseUrl} onClick={() => handleTestProvider(newProvider().baseUrl, newProvider().apiKeyEntries[0]?.apiKey || "", parseHeaders(headersInput()))} size="sm" variant="secondary">{testingNewProvider() ? t("apiKeys.testing") : t("apiKeys.actions.testConnection")}</Button>
            <Button onClick={editingIndex() !== null ? resetForm : () => local.setShowAddForm(false)} size="sm" variant="ghost">{t("common.cancel")}</Button>
          </div>
          <Show when={testResult()}>
            <div class={`rounded-lg p-2 text-sm ${testResult()?.success ? "bg-green-50 text-green-700 dark:bg-green-900/20 dark:text-green-300" : "bg-red-50 text-red-700 dark:bg-red-900/20 dark:text-red-300"}`}>
              <Show when={testResult()?.success} fallback={<span>{t("apiKeys.connectionFailedWithMessage", { message: testResult()?.message || "" })}</span>}>
                <span>{t("apiKeys.connectionSuccessful")} {testResult()?.modelsFound ? t("apiKeys.foundModelsWithCount", { count: testResult()?.modelsFound || 0 }) : ""}</span>
              </Show>
            </div>
          </Show>
        </div>
      </Show>

      <Show when={!local.showAddForm()}>
        <Button class="w-full" disabled={!proxyStatus().running} onClick={() => local.setShowAddForm(true)} variant="secondary">{t("apiKeys.actions.addOllamaProvider")}</Button>
      </Show>

      <Show when={showModelManager()}>
        <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4">
          <div class="w-full max-w-lg space-y-4 rounded-xl bg-white p-6 shadow-xl dark:bg-gray-900">
            <div class="flex items-center justify-between"><h3 class="font-semibold text-gray-900 dark:text-gray-100">{t("apiKeys.actions.manageModels")}</h3><Button onClick={() => { setShowModelManager(false); setManagingProviderIndex(null); }} size="sm" variant="ghost">X</Button></div>
            <div class="flex gap-2">
              <input class="flex-1 rounded-lg border border-gray-300 bg-white px-3 py-2 text-sm focus:border-transparent focus:ring-2 focus:ring-brand-500 dark:border-gray-600 dark:bg-gray-900" onInput={(e) => setNewModelInput(e.currentTarget.value)} onKeyDown={(e) => { if (e.key === "Enter") handleAddModel(); }} placeholder={t("apiKeys.placeholders.modelName")} type="text" value={newModelInput()} />
              <Button disabled={!newModelInput().trim()} onClick={handleAddModel} size="sm" variant="primary">{t("common.add")}</Button>
              <Button disabled={fetchingModels()} onClick={handleFetchModels} size="sm" title={t("apiKeys.actions.fetchModelsFromProvider")} variant="secondary">{fetchingModels() ? t("apiKeys.fetching") : t("apiKeys.actions.fetch")}</Button>
            </div>
            <div class="max-h-60 space-y-2 overflow-y-auto">
              <Show when={managingProviderIndex() !== null && (providers()[managingProviderIndex()!]?.models || []).length > 0}>
                <For each={providers()[managingProviderIndex()!]?.models || []}>{(model, index) => <div class="flex items-center justify-between rounded-lg border border-gray-200 bg-gray-50 p-2 dark:border-gray-700 dark:bg-gray-800/50"><span class="text-sm text-gray-700 dark:text-gray-300">{model.name}</span><Button onClick={() => handleRemoveModel(index())} size="sm" variant="ghost">X</Button></div>}</For>
              </Show>
              <Show when={managingProviderIndex() !== null && (providers()[managingProviderIndex()!]?.models || []).length === 0}><p class="py-4 text-center text-sm text-gray-500 dark:text-gray-400">{t("apiKeys.noModelsAddedYet")}</p></Show>
            </div>
            <div class="flex justify-end gap-2 pt-2"><Button onClick={() => { setShowModelManager(false); setManagingProviderIndex(null); }} size="sm" variant="ghost">{t("common.cancel")}</Button><Button disabled={local.loading()} onClick={handleSaveModels} size="sm" variant="primary">{t("common.save")}</Button></div>
          </div>
        </div>
      </Show>

      <Show when={showEmptyState()}>
        <div class="py-8 text-center text-gray-500 dark:text-gray-400"><p class="text-sm">{t("apiKeys.noApiKeysConfiguredYet")}</p><p class="mt-1 text-xs">{t("apiKeys.addFirstKeyHint")}</p></div>
      </Show>
    </div>
  );
}
