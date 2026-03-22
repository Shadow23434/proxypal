import { createSignal, Show } from "solid-js";
import { Button } from "./ui";
import { useI18n } from "../i18n";

interface GeminiWebCookieModalProps {
  loading?: boolean;
  onCancel: () => void;
  onSubmit: (payload: {
    label?: string;
    secure1psid: string;
    secure1psidts: string;
  }) => Promise<void>;
  open: boolean;
}

export function GeminiWebCookieModal(props: GeminiWebCookieModalProps) {
  const { t } = useI18n();
  const [secure1psid, setSecure1psid] = createSignal("");
  const [secure1psidts, setSecure1psidts] = createSignal("");
  const [label, setLabel] = createSignal("");
  const [error, setError] = createSignal("");

  const handleSubmit = async () => {
    const psid = secure1psid().trim();
    const psidts = secure1psidts().trim();
    const accountLabel = label().trim();

    if (!psid || !psidts) {
      setError(t("oauth.geminiWebCookieRequired"));
      return;
    }

    setError("");
    await props.onSubmit({
      label: accountLabel || undefined,
      secure1psid: psid,
      secure1psidts: psidts,
    });
  };

  return (
    <Show when={props.open}>
      <div
        class="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4 backdrop-blur-sm"
        onClick={(e) => e.target === e.currentTarget && props.onCancel()}
      >
        <div class="w-full max-w-lg rounded-xl border border-gray-200 bg-white shadow-xl dark:border-gray-700 dark:bg-gray-800">
          <div class="border-b border-gray-100 px-5 pb-4 pt-5 dark:border-gray-700">
            <div class="flex items-center gap-3">
              <div class="flex h-10 w-10 items-center justify-center overflow-hidden rounded-lg bg-gray-100 dark:bg-gray-700">
                <img alt="Gemini Web" class="h-7 w-7 object-contain" src="/logos/gemini.svg" />
              </div>
              <div>
                <h3 class="font-semibold text-gray-900 dark:text-gray-100">
                  {t("oauth.connect", { provider: "Gemini Web" })}
                </h3>
                <p class="text-xs text-gray-500 dark:text-gray-400">
                  {t("oauth.geminiWebCookieSubtitle")}
                </p>
              </div>
            </div>
          </div>

          <div class="space-y-4 p-5">
            <div class="rounded-lg border border-sky-200 bg-sky-50 px-3 py-2.5 dark:border-sky-800 dark:bg-sky-900/20">
              <p class="text-xs text-sky-700 dark:text-sky-300">
                {t("oauth.geminiWebCookieHint")}
              </p>
            </div>

            <div class="space-y-1.5">
              <label class="text-xs font-medium uppercase tracking-wide text-gray-500 dark:text-gray-400">
                Label
              </label>
              <input
                class="w-full rounded-lg border border-gray-200 bg-gray-50 px-3 py-2.5 text-sm text-gray-900 outline-none transition focus:border-brand-500 dark:border-gray-600 dark:bg-gray-900 dark:text-gray-100"
                onInput={(e) => setLabel(e.currentTarget.value)}
                placeholder={t("oauth.geminiWebCookieLabelPlaceholder")}
                value={label()}
              />
            </div>

            <div class="space-y-1.5">
              <label class="text-xs font-medium uppercase tracking-wide text-gray-500 dark:text-gray-400">
                __Secure-1PSID
              </label>
              <textarea
                class="min-h-20 w-full rounded-lg border border-gray-200 bg-gray-50 px-3 py-2.5 font-mono text-xs text-gray-900 outline-none transition focus:border-brand-500 dark:border-gray-600 dark:bg-gray-900 dark:text-gray-100"
                onInput={(e) => setSecure1psid(e.currentTarget.value)}
                placeholder="Paste __Secure-1PSID"
                value={secure1psid()}
              />
            </div>

            <div class="space-y-1.5">
              <label class="text-xs font-medium uppercase tracking-wide text-gray-500 dark:text-gray-400">
                __Secure-1PSIDTS
              </label>
              <textarea
                class="min-h-20 w-full rounded-lg border border-gray-200 bg-gray-50 px-3 py-2.5 font-mono text-xs text-gray-900 outline-none transition focus:border-brand-500 dark:border-gray-600 dark:bg-gray-900 dark:text-gray-100"
                onInput={(e) => setSecure1psidts(e.currentTarget.value)}
                placeholder="Paste __Secure-1PSIDTS"
                value={secure1psidts()}
              />
            </div>

            <Show when={error()}>
              <div class="rounded-lg border border-red-200 bg-red-50 px-3 py-2 text-sm text-red-700 dark:border-red-800 dark:bg-red-900/20 dark:text-red-300">
                {error()}
              </div>
            </Show>
          </div>

          <div class="flex gap-2 border-t border-gray-100 px-5 pb-5 pt-3 dark:border-gray-700">
            <Button class="flex-1" loading={props.loading} onClick={handleSubmit} variant="primary">
              {t("oauth.geminiWebSaveCookies")}
            </Button>
            <Button class="flex-1" onClick={props.onCancel} variant="secondary">
              {t("common.cancel")}
            </Button>
          </div>
        </div>
      </div>
    </Show>
  );
}
