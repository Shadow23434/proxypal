import { createSignal, For, onMount, Show } from "solid-js";
import { useI18n } from "../../i18n";
import {
  configureCliAgent,
  detectCliAgents,
  getAvailableModels,
  testProviderConnection,
} from "../../lib/tauri";
import { appStore } from "../../stores/app";
import { toastStore } from "../../stores/toast";
import { ModelsWidget } from "../ModelsWidget";
import { Button } from "../ui";

import type { AgentStatus, AppConfig, AvailableModel } from "../../lib/tauri";

type ModelTestStatus = "untested" | "testing" | "available" | "failed";

export interface ModelStatusInfo {
  latencyMs?: number;
  message?: string;
  status: ModelTestStatus;
  testedAt?: number;
}

interface ModelsSettingsProps {
  config: AppConfig;
  setConfig: (updater: (prev: AppConfig) => AppConfig) => void;
}

export function ModelsSettings(props: ModelsSettingsProps) {
  const { t } = useI18n();
  void props;

  const [models, setModels] = createSignal<AvailableModel[]>([]);
  const [agents, setAgents] = createSignal<AgentStatus[]>([]);
  const [configuringAgent, setConfiguringAgent] = createSignal<string | null>(null);
  const [modelStatuses, setModelStatuses] = createSignal<Record<string, ModelStatusInfo>>({});
  const [modelsLoading, setModelsLoading] = createSignal(false);
  let refreshModelsPromise: Promise<void> | null = null;

  const refreshModels = async () => {
    if (!appStore.proxyStatus().running) {
      setModelsLoading(false);
      return;
    }
    if (refreshModelsPromise) {
      return refreshModelsPromise;
    }

    setModelsLoading(true);
    refreshModelsPromise = (async () => {
      try {
        const availableModels = await getAvailableModels();
        setModels(availableModels);
        setModelStatuses((prev) => {
          const next: Record<string, ModelStatusInfo> = {};
          for (const model of availableModels) {
            if (prev[model.id]) {
              next[model.id] = prev[model.id];
            }
          }
          return next;
        });
      } catch (error) {
        console.error("Failed to load models:", error);
      } finally {
        setModelsLoading(false);
        refreshModelsPromise = null;
      }
    })();

    return refreshModelsPromise;
  };

  onMount(async () => {
    await refreshModels();

    // Load agents
    try {
      const agentList = await detectCliAgents();
      setAgents(agentList);
    } catch (error) {
      console.error("Failed to load agents:", error);
    }
  });

  const testModel = async (modelId: string) => {
    const currentStatus = modelStatuses()[modelId]?.status;
    if (currentStatus === "testing") {
      return;
    }

    setModelStatuses((prev) => ({
      ...prev,
      [modelId]: {
        ...prev[modelId],
        status: "testing",
      },
    }));

    try {
      const result = await testProviderConnection(modelId);
      setModelStatuses((prev) => ({
        ...prev,
        [modelId]: {
          latencyMs: result.latencyMs,
          message: result.message,
          status: result.success ? "available" : "failed",
          testedAt: Date.now(),
        },
      }));
    } catch (error) {
      setModelStatuses((prev) => ({
        ...prev,
        [modelId]: {
          message: error instanceof Error ? error.message : String(error),
          status: "failed",
          testedAt: Date.now(),
        },
      }));
    } finally {
      await refreshModels();
    }
  };

  const handleConfigureAgent = async (agentId: string) => {
    if (!appStore.proxyStatus().running) {
      toastStore.warning(
        t("settings.toasts.startProxyFirst"),
        t("settings.toasts.proxyMustBeRunningToConfigureAgents"),
      );
      return;
    }
    setConfiguringAgent(agentId);
    try {
      const availableModels = await getAvailableModels();
      const result = await configureCliAgent(agentId, availableModels);
      const agent = agents().find((a) => a.id === agentId);
      if (result.success) {
        const refreshed = await detectCliAgents();
        setAgents(refreshed);
        toastStore.success(
          t("settings.toasts.agentConfigured", {
            name: agent?.name || agentId,
          }),
        );
      }
    } catch (error) {
      console.error("Failed to configure agent:", error);
      toastStore.error(t("settings.toasts.configurationFailed"), String(error));
    } finally {
      setConfiguringAgent(null);
    }
  };

  return (
    <>
      {/* Available Models */}
      <div class="space-y-4">
        <h2 class="text-sm font-semibold uppercase tracking-wider text-gray-600 dark:text-gray-400">
          Available Models
        </h2>
        <ModelsWidget
          loading={modelsLoading()}
          modelStatuses={modelStatuses()}
          models={models()}
          onTestModel={testModel}
        />
      </div>

      {/* CLI Agents */}
      <div class="space-y-4">
        <h2 class="text-sm font-semibold uppercase tracking-wider text-gray-600 dark:text-gray-400">
          CLI Agents
        </h2>
        <div class="divide-y divide-gray-100 rounded-xl border border-gray-200 bg-white dark:divide-gray-700 dark:border-gray-700 dark:bg-gray-800">
          <For each={agents()}>
            {(agent) => (
              <div class="flex items-center justify-between p-3">
                <div class="flex items-center gap-3">
                  <Show when={agent.logo}>
                    <img alt={agent.name} class="h-6 w-6 rounded" src={agent.logo} />
                  </Show>
                  <div>
                    <div class="flex items-center gap-2">
                      <span class="text-sm font-medium text-gray-900 dark:text-gray-100">
                        {agent.name}
                      </span>
                      <Show when={agent.configured}>
                        <span class="rounded bg-green-100 px-1.5 py-0.5 text-[10px] font-medium text-green-700 dark:bg-green-800 dark:text-green-300">
                          Configured
                        </span>
                      </Show>
                    </div>
                    <p class="text-xs text-gray-500 dark:text-gray-400">{agent.description}</p>
                  </div>
                </div>
                <Button
                  disabled={configuringAgent() === agent.id}
                  onClick={() => handleConfigureAgent(agent.id)}
                  size="sm"
                  variant={agent.configured ? "secondary" : "primary"}
                >
                  <Show
                    fallback={
                      <svg class="h-4 w-4 animate-spin" fill="none" viewBox="0 0 24 24">
                        <circle
                          class="opacity-25"
                          cx="12"
                          cy="12"
                          r="10"
                          stroke="currentColor"
                          stroke-width="4"
                        />
                        <path
                          class="opacity-75"
                          d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"
                          fill="currentColor"
                        />
                      </svg>
                    }
                    when={configuringAgent() !== agent.id}
                  >
                    {agent.configured ? "Reconfigure" : "Configure"}
                  </Show>
                </Button>
              </div>
            )}
          </For>
          <Show when={agents().length === 0}>
            <div class="p-4 text-center text-sm text-gray-500 dark:text-gray-400">
              No CLI agents detected
            </div>
          </Show>
        </div>
      </div>
    </>
  );
}
